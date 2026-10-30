use super::*;
use crate::{pe::Symbol, win32::ServiceError};
use std::collections::BTreeMap;

pub(super) struct Descriptor {
    pub owner: u64,
    pub dll: String,
    pub bound: Option<u64>,
}

pub(super) struct Thunk {
    pub descriptor: u64,
    pub symbol: Symbol,
    iat: u64,
    target: Option<u64>,
}

#[derive(Default)]
pub(super) struct Registry {
    descriptors: BTreeMap<u64, Descriptor>,
    thunks: BTreeMap<u64, Thunk>,
}

impl Registry {
    pub fn remaining(&self) -> usize {
        4096 - self.thunks.len()
    }

    pub fn stage(&self, modules: &mut Linked, memory: &mut Memory) -> Result<Self, String> {
        let width = memory.mode() as usize / 8;
        let mut slots = gateways::delay_area(memory.mode())
            .step_by(16)
            .filter(|pc| !self.thunks.contains_key(pc));
        let mut staged = Self::default();
        let mut outputs = BTreeSet::new();
        let mut writes = Vec::new();
        let mut output = |address, value| -> Result<(), String> {
            memory.check(address, width, WRITE)?;
            for byte in address..address + width as u64 {
                if !outputs.insert(byte) {
                    return Err("overlapping PE delay outputs".into());
                }
            }
            writes.push((address, value));
            Ok(())
        };
        for module in modules.modules.values_mut() {
            for delay in std::mem::take(&mut module.delays) {
                if delay.functions.is_empty() {
                    continue;
                }
                let descriptor = module.base + u64::from(delay.handle);
                output(descriptor, 0)?;
                let terminator = delay.functions.last().unwrap().iat_rva as u64 + width as u64;
                output(module.base + terminator, 0)?;
                if let Some(unload) = delay.unload {
                    output(
                        module.base + u64::from(unload) + (delay.functions.len() * width) as u64,
                        0,
                    )?;
                }
                staged.descriptors.insert(
                    descriptor,
                    Descriptor {
                        owner: module.base,
                        dll: delay.dll,
                        bound: None,
                    },
                );
                for (index, function) in delay.functions.into_iter().enumerate() {
                    let pc = slots.next().ok_or("PE delay import capacity exceeded")?;
                    let iat = module.base + u64::from(function.iat_rva);
                    output(iat, pc)?;
                    if let Some(unload) = delay.unload {
                        output(module.base + u64::from(unload) + (index * width) as u64, pc)?;
                    }
                    let symbol = match (function.name, function.ordinal) {
                        (Some(name), None) => Symbol::Name(name),
                        (None, Some(ordinal)) => Symbol::Ordinal(ordinal),
                        _ => return Err("PE delay import has no identity".into()),
                    };
                    staged.thunks.insert(
                        pc,
                        Thunk {
                            descriptor,
                            symbol,
                            iat,
                            target: None,
                        },
                    );
                }
            }
        }
        for (at, value) in writes {
            memory.write_int(at, width, value)?;
        }
        Ok(staged)
    }

    pub fn commit(&mut self, staged: Self) {
        self.descriptors.extend(staged.descriptors);
        self.thunks.extend(staged.thunks);
    }

    pub fn thunk(&self, pc: u64) -> Option<&Thunk> {
        self.thunks.get(&pc)
    }

    pub fn descriptor(&self, pc: u64) -> &Descriptor {
        &self.descriptors[&self.thunks[&pc].descriptor]
    }

    pub fn binding(&mut self, pc: u64, base: u64) {
        let key = self.thunks[&pc].descriptor;
        self.descriptors.get_mut(&key).unwrap().bound = Some(base);
    }

    pub fn preflight(&self, pc: u64, memory: &Memory) -> Result<(), Fault> {
        let thunk = &self.thunks[&pc];
        let width = memory.mode() as usize / 8;
        memory.check(thunk.descriptor, width, WRITE)?;
        memory.check(thunk.iat, width, WRITE)
    }

    pub fn publish(
        &mut self,
        pc: u64,
        base: u64,
        target: u64,
        memory: &mut Memory,
    ) -> Result<(), Fault> {
        self.preflight(pc, memory)?;
        let thunk = self.thunks.get_mut(&pc).unwrap();
        let width = memory.mode() as usize / 8;
        memory.write_int(thunk.descriptor, width, base)?;
        memory.write_int(thunk.iat, width, target)?;
        thunk.target = Some(target);
        Ok(())
    }

    pub fn reset_writes(
        &self,
        invalid: &BTreeSet<u64>,
        removing: &BTreeSet<u64>,
    ) -> Vec<(u64, u64)> {
        let selected: BTreeSet<_> = self
            .descriptors
            .iter()
            .filter_map(|(&key, descriptor)| {
                (!removing.contains(&descriptor.owner)
                    && descriptor.bound.is_some_and(|base| invalid.contains(&base)))
                .then_some(key)
            })
            .collect();
        selected
            .iter()
            .map(|&key| (key, 0))
            .chain(
                self.thunks
                    .iter()
                    .filter(|(_, t)| selected.contains(&t.descriptor))
                    .map(|(&pc, t)| (t.iat, pc)),
            )
            .collect()
    }

    pub fn invalidate(&mut self, invalid: &BTreeSet<u64>, removing: &BTreeSet<u64>) {
        self.descriptors.retain(|_, d| !removing.contains(&d.owner));
        for descriptor in self.descriptors.values_mut() {
            if descriptor.bound.is_some_and(|base| invalid.contains(&base)) {
                descriptor.bound = None;
            }
        }
        self.thunks
            .retain(|_, t| self.descriptors.contains_key(&t.descriptor));
        for thunk in self.thunks.values_mut() {
            if self.descriptors[&thunk.descriptor].bound.is_none() {
                thunk.target = None;
            }
        }
    }
}

impl Loaded {
    pub(crate) fn is_delay(&self, pc: u64) -> bool {
        self.delays.thunk(pc).is_some()
    }

    pub(crate) fn begin_delay(
        &mut self,
        state: &mut State,
        memory: &mut Memory,
    ) -> Result<(), ServiceError> {
        let pc = state.ip;
        if !self.is_delay(pc) {
            return Err(ServiceError::Unbound);
        }
        let thunk = self.delays.thunk(pc).unwrap();
        self.delays.preflight(pc, memory)?;
        if let Some(target) = thunk.target {
            let base = self.delays.descriptor(pc).bound.unwrap();
            self.delays.publish(pc, base, target, memory)?;
            state.ip = target;
            return Ok(());
        }
        if self.frames.len() >= 32 {
            return Err(ServiceError::Unavailable(
                "delay resolution nesting capacity exceeded",
            ));
        }
        let root = self.delays.descriptor(pc).bound.unwrap_or(0);
        let owner = self.delays.descriptor(pc).owner;
        let task = super::symbols::Resolve::delay(pc, root, thunk.symbol.clone());
        self.frames.push(modules::Frame::resolving(
            Box::new(state.clone()),
            task,
            owner,
        ));
        state.ip = gateways::layout(memory.mode()).callback;
        Ok(())
    }
}
