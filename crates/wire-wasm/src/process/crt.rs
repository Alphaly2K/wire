//! CRT callbacks execute in the same JIT and retain progress at every faultable boundary.
use super::{
    callbacks::{CallFrame, Callback},
    unwind, *,
};
use crate::win32::{
    call::Call,
    crt_sort::{Action, Sort},
    ServiceError,
};
#[derive(Clone)]
enum Operation {
    Init {
        next: u64,
        end: u64,
        checked: bool,
    },
    Sort(Sort),
    Exit {
        dll: String,
        quick: bool,
        code: Option<u32>,
    },
}
#[derive(Clone)]
pub(crate) struct Frame {
    pub(super) saved: Box<State>,
    pub(super) loader_depth: usize,
    call: Call,
    operation: Operation,
    return_sp: u64,
    waiting: bool,
    returned: Option<u32>,
}
enum Finish {
    Entered,
    Return(u32),
    Exit(u32),
}
impl Frame {
    pub(super) fn unwind_bridge(&self) -> unwind::Bridge {
        let mut caller = self.saved.clone();
        self.call.clone().complete(&mut caller, 0);
        unwind::Bridge {
            sp: self.return_sp,
            caller: Some((caller, true)),
        }
    }
    fn enter(
        &mut self,
        target: u64,
        args: &[u64],
        state: &mut State,
        memory: &mut Memory,
    ) -> Result<(), Fault> {
        let entry = CallFrame::new(Mode::X86, self.saved.regs[4], args)?;
        let return_sp = entry.sp + 4;
        entry.enter(target, state, memory)?;
        self.return_sp = return_sp;
        self.waiting = true;
        Ok(())
    }
    fn drive(
        &mut self,
        crt: &mut crate::win32::crt::Crt,
        state: &mut State,
        memory: &mut Memory,
    ) -> Result<Finish, StartupError> {
        if let Some(result) = self.returned.take() {
            self.waiting = false;
            match &mut self.operation {
                Operation::Init { checked: true, .. } if result != 0 => {
                    return Ok(Finish::Return(result))
                }
                Operation::Sort(sort) => sort.compared(result as i32),
                _ => {}
            }
        }
        loop {
            match &mut self.operation {
                Operation::Init { next, end, .. } => {
                    if *next == *end {
                        return Ok(Finish::Return(0));
                    }
                    let target = memory.read_int(*next, 4)?;
                    if target == 0 {
                        *next += 4;
                        continue;
                    }
                    self.enter(target, &[], state, memory)?;
                    if let Operation::Init { next, .. } = &mut self.operation {
                        *next += 4;
                    }
                    return Ok(Finish::Entered);
                }
                Operation::Sort(sort) => match sort.next() {
                    Action::Done => return Ok(Finish::Return(0)),
                    Action::Compare(a, b) => {
                        let target = u64::from(sort.compare);
                        self.enter(target, &[u64::from(a), u64::from(b)], state, memory)?;
                        return Ok(Finish::Entered);
                    }
                    Action::Swap(a, b) => {
                        let width = sort.width as usize;
                        memory.check(u64::from(a), width, READ | WRITE)?;
                        memory.check(u64::from(b), width, READ | WRITE)?;
                        let mut first = vec![0; width];
                        let mut second = first.clone();
                        memory.read(u64::from(a), &mut first)?;
                        memory.read(u64::from(b), &mut second)?;
                        memory.write(u64::from(a), &second)?;
                        memory.write(u64::from(b), &first)?;
                        sort.swapped();
                    }
                },
                Operation::Exit { dll, quick, code } => {
                    let handlers = if *quick {
                        &mut crt.quick
                    } else {
                        &mut crt.normal
                    };
                    let Some(target) = handlers.get(dll).and_then(|v| v.last()).copied() else {
                        return Ok(code.map_or(Finish::Return(0), Finish::Exit));
                    };
                    let dll = dll.clone();
                    let quick = *quick;
                    self.enter(target, &[], state, memory)?;
                    let handlers = if quick {
                        &mut crt.quick
                    } else {
                        &mut crt.normal
                    };
                    handlers.get_mut(&dll).unwrap().pop();
                    return Ok(Finish::Entered);
                }
            }
        }
    }
}
impl Loaded {
    pub(crate) fn crt_callback(
        &mut self,
        name: &str,
        dll: &str,
        call: Call,
        state: &mut State,
        memory: &mut Memory,
    ) -> Result<(), ServiceError> {
        if self.threads.current().callbacks.len() >= 32 {
            return Err(ServiceError::Unavailable(
                "CRT callback nesting capacity exceeded",
            ));
        }
        let a = &call.arguments;
        let operation = match name {
            "_initterm" | "_initterm_e" => {
                if a[0] > a[1] || !(a[1] - a[0]).is_multiple_of(4) || a[1] - a[0] > 16384 {
                    return Err(ServiceError::Unavailable("invalid CRT initializer array"));
                }
                Operation::Init {
                    next: a[0],
                    end: a[1],
                    checked: name == "_initterm_e",
                }
            }
            "qsort" => {
                let sort = Sort::new(a[0] as u32, a[1] as u32, a[2] as u32, a[3] as u32)
                    .map_err(ServiceError::Unavailable)?;
                if sort.size() != 0 {
                    memory.check(a[0], sort.size() as usize, READ | WRITE)?;
                }
                Operation::Sort(sort)
            }
            "exit" | "_exit" | "quick_exit" | "_cexit" => {
                if name != "_cexit" && !self.can_exit_thread() {
                    return Err(ServiceError::Unavailable(
                        "CRT process exit during loader or shutdown is not implemented",
                    ));
                }
                if self.threads.snapshot().any(|(id,_)| self.threads.get(id).unwrap().callbacks.iter().any(|c| matches!(c,Callback::Crt(f) if matches!(&f.operation,Operation::Exit {dll: d,..} if d == dll)))) { return Err(ServiceError::Unavailable("recursive CRT shutdown is not implemented")); }
                if name == "_exit" {
                    self.begin_process_exit(a[0] as u32, state, Mode::X86)?;
                    return Ok(());
                }
                Operation::Exit {
                    dll: dll.into(),
                    quick: name == "quick_exit",
                    code: (name != "_cexit").then_some(a[0] as u32),
                }
            }
            _ => unreachable!("CRT callback export classification"),
        };
        let mut frame = Frame {
            saved: Box::new(state.clone()),
            loader_depth: self.callback_depth(),
            call,
            operation,
            return_sp: 0,
            waiting: false,
            returned: None,
        };
        let mut entered = state.clone();
        match frame
            .drive(&mut self.crt, &mut entered, memory)
            .map_err(|e| match e {
                StartupError::Fault(f) => ServiceError::Fault(f),
                _ => ServiceError::Unavailable("CRT callback setup failed"),
            })? {
            Finish::Entered => self
                .threads
                .current_mut()
                .callbacks
                .push(Callback::Crt(Box::new(frame))),
            Finish::Return(value) => frame.call.complete(&mut entered, u64::from(value)),
            Finish::Exit(code) => self.begin_process_exit(code, &mut entered, Mode::X86)?,
        }
        *state = entered;
        Ok(())
    }
    pub(super) fn resume_crt(
        &mut self,
        state: &mut State,
        memory: &mut Memory,
    ) -> Result<(), StartupError> {
        let Some(Callback::Crt(mut frame)) = self.threads.current_mut().callbacks.pop() else {
            return Err(unwind::invalid("CRT return has no continuation"));
        };
        if frame.waiting && frame.returned.is_none() {
            frame.returned = Some(state.regs[0] as u32);
        }
        let mut restored = *frame.saved.clone();
        restored.work = state.work;
        restored.retired = state.retired;
        match frame.drive(&mut self.crt, &mut restored, memory) {
            Ok(Finish::Entered) => self
                .threads
                .current_mut()
                .callbacks
                .push(Callback::Crt(frame)),
            Ok(Finish::Return(value)) => frame.call.complete(&mut restored, u64::from(value)),
            Ok(Finish::Exit(code)) => self
                .begin_process_exit(code, &mut restored, Mode::X86)
                .map_err(|_| unwind::invalid("CRT process shutdown continuation failed"))?,
            Err(error) => {
                self.threads
                    .current_mut()
                    .callbacks
                    .push(Callback::Crt(frame));
                return Err(error);
            }
        }
        *state = restored;
        memory.clear_fault();
        Ok(())
    }
}
