//! Balance Unity's actual temporary BDWGC suppression, including probe unwind.
//! No managed allocation/GC checks may claim success while collection is disabled.
use super::Object;
#[derive(Clone, Copy)]
struct Api {
    disable: unsafe extern "C" fn(),
    enable: unsafe extern "C" fn(),
    disabled: unsafe extern "C" fn() -> i32,
}
impl Api {
    fn load(library: Object) -> Result<Self, String> {
        macro_rules! sym {
            ($name:literal, $ty:ty) => {{
                let p = unsafe { libc::dlsym(library, concat!($name, "\0").as_ptr().cast()) };
                if p.is_null() {
                    return Err(concat!("missing Unity GC scope API: ", $name).into());
                }
                unsafe { std::mem::transmute::<Object, $ty>(p) }
            }};
        }
        Ok(Self {
            disable: sym!("mono_unity_gc_disable", unsafe extern "C" fn()),
            enable: sym!("mono_unity_gc_enable", unsafe extern "C" fn()),
            disabled: sym!("mono_unity_gc_is_disabled", unsafe extern "C" fn() -> i32),
        })
    }
}
#[derive(Default)]
pub(super) struct State {
    api: Option<Api>,
    depth: usize,
    disables: usize,
    enables: usize,
    unwound: usize,
}
impl State {
    fn check(&self, api: Api) -> Result<(), String> {
        if (unsafe { (api.disabled)() } != 0) != (self.depth != 0) {
            return Err("Unity GC state differs from owned suppression scopes".into());
        }
        Ok(())
    }
    pub(super) fn dispatch(&mut self, library: Object, function: &str) -> Result<u64, String> {
        let api = match self.api {
            Some(api) => api,
            None => {
                let api = Api::load(library)?;
                self.api = Some(api);
                api
            }
        };
        self.apply(api, function)
    }
    fn apply(&mut self, api: Api, function: &str) -> Result<u64, String> {
        self.check(api)?;
        match function {
            "mono_unity_gc_disable" => {
                if self.depth >= 32 {
                    return Err("Unity GC suppression nesting exceeds bound".into());
                }
                unsafe { (api.disable)() };
                self.depth += 1;
                self.disables += 1;
            }
            "mono_unity_gc_enable" => {
                if self.depth == 0 {
                    return Err("unbalanced Unity GC enable".into());
                }
                unsafe { (api.enable)() };
                self.depth -= 1;
                self.enables += 1;
            }
            "mono_unity_gc_is_disabled" => return Ok(u64::from(self.depth != 0)),
            _ => return Err("unadmitted Unity GC scope operation".into()),
        }
        self.check(api)?;
        Ok(0)
    }
    /// Diagnostic exit can occur inside a real Unity scope. Re-enable before
    /// forcing collection, releasing roots, or shutting down the runtime.
    pub(super) fn restore(&mut self) -> Result<(), String> {
        let Some(api) = self.api else {
            return Ok(());
        };
        self.check(api)?;
        while self.depth != 0 {
            unsafe { (api.enable)() };
            self.depth -= 1;
            self.unwound += 1;
        }
        self.check(api)
    }
    pub(super) fn report(&self) -> String {
        format!("native-gc-scope-disables={}\nnative-gc-scope-enables={}\nnative-gc-scope-unwound={}\nnative-gc-scope-depth={}\n", self.disables, self.enables, self.unwound, self.depth)
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::Cell;
    thread_local! { static DEPTH: Cell<usize> = const { Cell::new(0) }; }
    unsafe extern "C" fn disable() {
        DEPTH.with(|c| c.set(c.get() + 1));
    }
    unsafe extern "C" fn enable() {
        DEPTH.with(|c| c.set(c.get() - 1));
    }
    unsafe extern "C" fn disabled() -> i32 {
        i32::from(DEPTH.with(Cell::get) != 0)
    }
    #[test]
    fn nested_scopes_use_real_transitions_and_restore_before_collection() {
        DEPTH.with(|c| c.set(0));
        let api = Api {
            disable,
            enable,
            disabled,
        };
        let mut state = State {
            api: Some(api),
            ..State::default()
        };
        assert!(state.apply(api, "mono_unity_gc_enable").is_err());
        state.apply(api, "mono_unity_gc_disable").unwrap();
        state.apply(api, "mono_unity_gc_disable").unwrap();
        assert_eq!(DEPTH.with(Cell::get), 2);
        state.apply(api, "mono_unity_gc_enable").unwrap();
        assert_eq!(state.apply(api, "mono_unity_gc_is_disabled").unwrap(), 1);
        state.restore().unwrap();
        assert_eq!(DEPTH.with(Cell::get), 0);
        assert_eq!(state.unwound, 1);
        state.restore().unwrap();
        for _ in 0..32 {
            state.apply(api, "mono_unity_gc_disable").unwrap();
        }
        assert!(state.apply(api, "mono_unity_gc_disable").is_err());
        assert_eq!(DEPTH.with(Cell::get), 32);
        state.restore().unwrap();
        DEPTH.with(|c| c.set(1));
        assert!(state.apply(api, "mono_unity_gc_is_disabled").is_err());
        DEPTH.with(|c| c.set(0));
    }
}
