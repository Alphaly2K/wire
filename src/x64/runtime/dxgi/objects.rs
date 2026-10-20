//! Graphics owns COM table/object allocation and recursive resource retirement.
use super::*;
use crate::x64::runtime::gateways::Gateways;

impl Graphics {
    pub(super) fn attach(&mut self, graphics: NativeGraphics) -> Result<(), String> {
        if self.backend.is_some() || !self.objects.is_empty() {
            return Err("Win64 graphics backend already attached".into());
        }
        self.backend = Some(graphics);
        Ok(())
    }
    fn table(
        &mut self,
        mem: &mut Memory,
        gateways: &mut Gateways,
        next: &mut u64,
        kind: Kind,
    ) -> Result<u64, String> {
        if let Some(&address) = self.tables.get(&kind) {
            return Ok(address);
        }
        let mut bytes = Vec::new();
        let methods: Vec<_> = match kind {
            Kind::Factory => COMMON.iter().chain(FACTORY).map(|(n, _, _)| *n).collect(),
            Kind::Adapter => COMMON.iter().chain(ADAPTER).map(|(n, _, _)| *n).collect(),
            Kind::Output => COMMON
                .iter()
                .chain(output::METHODS)
                .map(|(n, _, _)| *n)
                .collect(),
            Kind::SwapChain => COMMON
                .iter()
                .chain(swapchain::METHODS)
                .map(|(n, _, _)| *n)
                .collect(),
            _ => d3d11::methods(kind),
        };
        for name in methods {
            bytes.extend_from_slice(&gateways.register(DLL, name).to_le_bytes());
        }
        let address = *next;
        mem.map(address, PAGE, READ | WRITE)?;
        mem.write(address, &bytes)?;
        mem.seal_runtime_readonly(address)?;
        *next += PAGE;
        self.tables.insert(kind, address);
        Ok(address)
    }
    pub(super) fn object(
        &mut self,
        mem: &mut Memory,
        gateways: &mut Gateways,
        next: &mut u64,
        kind: Kind,
        parent: u64,
    ) -> Result<u64, String> {
        if self.objects.len() >= OBJECT_LIMIT {
            return Err("graphics object capacity exceeded".into());
        }
        let table = self.table(mem, gateways, next, kind)?;
        if parent != 0 && self.objects.get(&parent).is_none_or(|o| o.refs == u32::MAX) {
            return Err("DXGI parent reference capacity".into());
        }
        let address = *next;
        mem.map(address, PAGE, READ | WRITE)?;
        mem.write_int(address, 8, table)?;
        mem.seal_runtime_readonly(address)?;
        if parent != 0 {
            self.retain(parent)?;
        }
        *next += PAGE;
        self.objects.insert(
            address,
            Object {
                kind,
                refs: 1,
                parent,
                association: if matches!(kind, Kind::Buffer | Kind::Texture) {
                    0x78000000
                } else {
                    0
                },
                private: BTreeMap::new(),
            },
        );
        Ok(address)
    }
    pub(super) fn release(&mut self, mem: &mut Memory, address: u64) -> Result<u64, String> {
        let o = self
            .objects
            .get_mut(&address)
            .ok_or("invalid DXGI object")?;
        if o.refs > 1 {
            o.refs -= 1;
            return Ok(o.refs as u64);
        }
        let parent = o.parent;
        mem.unmap(address, PAGE)?;
        self.objects.remove(&address);
        self.devices.remove(&address);
        self.queries.remove(&address);
        if let Some(chain) = self.swapchains.remove(&address) {
            self.release(mem, chain.buffer)?;
            self.release(mem, chain.device)?;
        }
        if let Some(d3d11::Resource::Buffer {
            mapped: Some((p, size, _)),
            ..
        }) = self.resources.get(&address)
        {
            mem.unmap(*p, *size)?;
        }
        if let Some(d3d11::Resource::Texture { mapped, .. }) = self.resources.get(&address) {
            for &(p, size, _) in mapped.values() {
                mem.unmap(p, size)?;
            }
        }
        if let Some(d3d11::Resource::View { resource, .. }) = self.resources.remove(&address) {
            self.release(mem, resource)?;
        }
        if let Some(context) = self.contexts.remove(&address) {
            for target in context.references() {
                self.release(mem, target)?;
            }
        }
        if parent != 0 {
            self.release(mem, parent)?;
        }
        Ok(0)
    }
    pub(super) fn adapter(
        &mut self,
        mem: &mut Memory,
        gateways: &mut Gateways,
        next: &mut u64,
        parent: u64,
        out: u64,
        iid: &Guid,
    ) -> Result<u64, String> {
        if !supports(Kind::Adapter, iid) {
            mem.write_int(out, 8, 0)?;
            return Ok(NO_INTERFACE);
        }
        let cached = self.objects.iter().find_map(|(&address, o)| {
            (o.kind == Kind::Adapter && o.parent == parent).then_some(address)
        });
        let address = if let Some(address) = cached {
            self.retain(address)?;
            address
        } else {
            if self.objects.len() >= OBJECT_LIMIT {
                mem.write_int(out, 8, 0)?;
                return Ok(OUT_OF_MEMORY);
            }
            self.object(mem, gateways, next, Kind::Adapter, parent)?
        };
        mem.write_int(out, 8, address)?;
        Ok(0)
    }
}
