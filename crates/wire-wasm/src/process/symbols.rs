use super::modules::{Finish, Frame};
use super::*;
use crate::{
    pe::{Export, Symbol},
    win32::{call::Call, context},
};

pub(crate) enum Lookup {
    Address {
        target: u64,
        edges: Vec<(u64, u64)>,
    },
    Missing {
        dll: String,
        from: u64,
        edges: Vec<(u64, u64)>,
    },
}

enum Completion {
    Api(Call),
    Delay(u64),
}

#[derive(Clone, Copy)]
enum Waiting {
    Primary,
    Forward(u64),
}

pub(super) struct Resolve {
    completion: Completion,
    root: u64,
    symbol: Symbol,
    waiting: Option<Waiting>,
    loaded: Option<Result<u64, u32>>,
    error: Option<u32>,
}

impl Resolve {
    pub(super) fn delay(pc: u64, root: u64, symbol: Symbol) -> Self {
        Self {
            completion: Completion::Delay(pc),
            root,
            symbol,
            waiting: None,
            loaded: None,
            error: None,
        }
    }
}

impl Loaded {
    pub(crate) fn lookup_export(
        &mut self,
        services: &mut Services,
        memory: &mut Memory,
        mut base: u64,
        mut symbol: Symbol,
        import: bool,
    ) -> Result<Lookup, u32> {
        let mut chain = BTreeSet::new();
        let mut edges = Vec::new();
        loop {
            if chain.len() >= 32 || !chain.insert((base, symbol.clone())) {
                return Err(127);
            }
            if let Some(dll) = services.provider_name(base).map(str::to_owned) {
                let name = symbol.provider_name();
                let target = if memory.mode() == Mode::X86
                    && crate::win32::crt::provider(&dll).is_some()
                    && crate::win32::crt_startup::data_export(&name)
                {
                    self.crt_data(services, memory, &dll)
                        .map_err(|_| 8u32)?
                        .cell(&name)
                        .ok_or_else(|| "unknown CRT data export".to_owned())
                } else if import {
                    services.bind(&dll, &name)
                } else {
                    services.lookup(&dll, &name)
                }
                .map_err(|_| 127u32)?;
                return Ok(Lookup::Address { target, edges });
            }
            let module = self.modules.modules.get(&base).ok_or(6u32)?;
            if module.phase == Phase::Failed {
                return Err(1114);
            }
            match module.exports.get(&symbol).cloned().ok_or(127u32)? {
                Export::Address(rva) => {
                    return Ok(Lookup::Address {
                        target: base + u64::from(rva),
                        edges,
                    })
                }
                Export::Forward {
                    dll,
                    symbol: forwarded,
                } => {
                    if linker::provider(&dll, services.mode()) {
                        let name = forwarded.provider_name();
                        let target = if memory.mode() == Mode::X86
                            && crate::win32::crt::provider(&dll).is_some()
                            && crate::win32::crt_startup::data_export(&name)
                        {
                            self.crt_data(services, memory, &dll)
                                .map_err(|_| 8u32)?
                                .cell(&name)
                                .ok_or_else(|| "unknown CRT data export".to_owned())
                        } else if import {
                            services.bind(&dll, &name)
                        } else {
                            services.lookup(&dll, &name)
                        }
                        .map_err(|_| 127u32)?;
                        return Ok(Lookup::Address { target, edges });
                    }
                    let next = self.modules.base(&dll);
                    if next == 0 {
                        return Ok(Lookup::Missing {
                            dll: dll.clone(),
                            from: base,
                            edges,
                        });
                    }
                    edges.push((base, next));
                    base = next;
                    symbol = forwarded.clone();
                }
            }
        }
    }

    pub(crate) fn remember_exports(&mut self, edges: Vec<(u64, u64)>) {
        for (from, to) in edges {
            let dependencies = &mut self.modules.modules.get_mut(&from).unwrap().dependencies;
            if !dependencies.contains(&to) {
                dependencies.push(to);
            }
        }
    }

    pub(crate) fn begin_resolution(
        &mut self,
        call: Call,
        root: u64,
        symbol: Symbol,
        state: &mut State,
        mode: Mode,
    ) -> Result<(), LoadError> {
        if self.frames.len() >= 32 {
            return Err(LoadError::Windows(8));
        }
        let task = Resolve {
            completion: Completion::Api(call),
            root,
            symbol,
            waiting: None,
            loaded: None,
            error: None,
        };
        self.frames
            .push(Frame::resolving(Box::new(state.clone()), task, root));
        state.ip = gateways::layout(mode).callback;
        Ok(())
    }

    pub(super) fn resolution_loaded(&mut self, result: Result<u64, u32>) {
        let Finish::Resolve(task) = &mut self.frames.last_mut().expect("resolution caller").finish
        else {
            unreachable!()
        };
        task.loaded = Some(result);
    }

    pub(super) fn resolution_loading(&mut self, base: u64) {
        let frame = self.frames.last_mut().expect("resolution caller");
        let Finish::Resolve(task) = &mut frame.finish else {
            unreachable!()
        };
        if matches!(task.waiting, Some(Waiting::Primary)) {
            task.root = base;
            let Completion::Delay(pc) = task.completion else {
                unreachable!()
            };
            self.delays.binding(pc, base);
        }
    }

    pub(super) fn advance_resolution(
        &mut self,
        mut frame: Frame,
        state: &mut State,
        memory: &mut Memory,
        services: &mut Services,
        files: &Files,
    ) -> Result<(), StartupError> {
        let Finish::Resolve(mut task) = std::mem::replace(&mut frame.finish, Finish::Resume) else {
            unreachable!()
        };
        if let Some(result) = task.loaded.take() {
            match result {
                Ok(base) => {
                    match task.waiting.take().unwrap() {
                        Waiting::Primary => task.root = base,
                        Waiting::Forward(from) => {
                            self.remember_exports(vec![(from, base)]);
                            self.modules.modules.get_mut(&base).unwrap().references -= 1;
                        }
                    }
                    frame.created.insert(base);
                }
                Err(code) => task.error = Some(code),
            }
        }
        let result = if let Some(code) = task.error {
            Err(code)
        } else if task.root == 0 {
            let Completion::Delay(pc) = task.completion else {
                unreachable!()
            };
            Ok(Lookup::Missing {
                dll: self.delays.descriptor(pc).dll.clone(),
                from: 0,
                edges: Vec::new(),
            })
        } else {
            self.lookup_export(
                services,
                memory,
                task.root,
                task.symbol.clone(),
                matches!(task.completion, Completion::Delay(_)),
            )
        };
        match result {
            Ok(Lookup::Missing { dll, from, edges }) => {
                frame
                    .created
                    .extend(edges.iter().flat_map(|&(a, b)| [a, b]));
                if from != 0 {
                    frame.created.insert(from);
                }
                self.remember_exports(edges);
                task.waiting = Some(if from == 0 {
                    Waiting::Primary
                } else {
                    Waiting::Forward(from)
                });
                frame.finish = Finish::Resolve(task);
                self.frames.push(frame);
                match self.start_load(&dll, Finish::Resume, state, memory, services, files) {
                    Ok(()) => Ok(()),
                    Err(LoadError::Fault(fault)) => Err(StartupError::Fault(fault)),
                    Err(LoadError::Windows(code)) => {
                        if let Some(request) = files.pending_input() {
                            return Err(StartupError::FileInput(request));
                        }
                        self.resolution_loaded(Err(code));
                        Ok(())
                    }
                }
            }
            result => {
                if let Completion::Delay(pc) = task.completion {
                    match result {
                        Ok(Lookup::Address { target, edges }) => {
                            if let Err(fault) = self.delays.publish(pc, task.root, target, memory) {
                                frame.finish = Finish::Resolve(task);
                                self.frames.push(frame);
                                return Err(StartupError::Fault(fault));
                            }
                            self.remember_exports(edges);
                            let (work, retired) = (state.work, state.retired);
                            *state = *frame.saved;
                            state.work = work;
                            state.retired = retired;
                            state.ip = target;
                            return Ok(());
                        }
                        Err(code) => {
                            let (work, retired) = (state.work, state.retired);
                            *state = *frame.saved;
                            state.work = work;
                            state.retired = retired;
                            return Err(StartupError::Invalid(format!(
                                "delay import resolution failed (Windows error {code})"
                            )));
                        }
                        _ => unreachable!(),
                    }
                }
                let value = match result {
                    Ok(Lookup::Address { target, edges }) => {
                        self.remember_exports(edges);
                        target
                    }
                    Err(code) => {
                        task.error = Some(code);
                        if let Err(fault) = memory.write_int(
                            context::error_address(&frame.saved, memory.mode()),
                            4,
                            u64::from(code),
                        ) {
                            frame.finish = Finish::Resolve(task);
                            self.frames.push(frame);
                            return Err(StartupError::Fault(fault));
                        }
                        0
                    }
                    _ => unreachable!(),
                };
                let (work, retired) = (state.work, state.retired);
                *state = *frame.saved;
                state.work = work;
                state.retired = retired;
                let Completion::Api(call) = task.completion else {
                    unreachable!()
                };
                call.complete(state, value);
                Ok(())
            }
        }
    }
}
