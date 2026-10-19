//! In-place bounded heapsort with real i386 guest comparator calls.
use crate::{
    api::{ApiLogger, ApiState, ApiTable},
    cpu::{Cpu, GuestCallOutcome},
    mem::GuestMem,
    thread::Sched,
};
struct Sort {
    base: u32,
    count: u32,
    width: u32,
    compare: u32,
}
impl Sort {
    fn pointer(&self, i: u32) -> u32 {
        self.base + i * self.width
    }
    fn swap(&self, mem: &GuestMem, a: u32, b: u32) -> Result<(), String> {
        let mut first = vec![0; self.width as usize];
        let mut second = first.clone();
        mem.read_bytes(self.pointer(a), &mut first)
            .map_err(|e| e.to_string())?;
        mem.read_bytes(self.pointer(b), &mut second)
            .map_err(|e| e.to_string())?;
        mem.write_bytes(self.pointer(a), &second)
            .map_err(|e| e.to_string())?;
        mem.write_bytes(self.pointer(b), &first)
            .map_err(|e| e.to_string())?;
        Ok(())
    }
    #[allow(clippy::too_many_arguments)]
    fn sift(
        &self,
        mut root: u32,
        end: u32,
        cpu: &mut Cpu,
        mem: &mut GuestMem,
        api: &mut ApiTable,
        state: &mut ApiState,
        log: &mut ApiLogger,
        sched: &mut Sched,
        limit: u64,
    ) -> Result<GuestCallOutcome, String> {
        macro_rules! compare {
            ($a:expr,$b:expr) => {
                match crate::x86::process::callbacks::invoke(
                    cpu,
                    mem,
                    api,
                    state,
                    log,
                    sched,
                    self.compare,
                    &[self.pointer($a), self.pointer($b)],
                    limit,
                )? {
                    GuestCallOutcome::Returned(n) => n as i32,
                    other => return Ok(other),
                }
            };
        }
        while root * 2 + 1 < end {
            let mut child = root * 2 + 1;
            if child + 1 < end && compare!(child, child + 1) < 0 {
                child += 1;
            }
            if compare!(root, child) >= 0 {
                break;
            }
            self.swap(mem, root, child)?;
            root = child;
        }
        Ok(GuestCallOutcome::Returned(0))
    }
}
#[allow(clippy::too_many_arguments)]
pub(crate) fn qsort(
    base: u32,
    count: u32,
    width: u32,
    compare: u32,
    cpu: &mut Cpu,
    mem: &mut GuestMem,
    api: &mut ApiTable,
    state: &mut ApiState,
    log: &mut ApiLogger,
    sched: &mut Sched,
    limit: u64,
) -> Result<GuestCallOutcome, String> {
    if count < 2 || width == 0 {
        return Ok(GuestCallOutcome::Returned(0));
    }
    let size = count.checked_mul(width).ok_or("qsort size overflow")?;
    if count > 1000000 || size > 64 * 1024 * 1024 || compare == 0 {
        return Err("qsort bounds or comparator invalid".into());
    }
    mem.ptr(base, size, true).map_err(|e| e.to_string())?;
    let sort = Sort {
        base,
        count,
        width,
        compare,
    };
    for start in (0..sort.count / 2).rev() {
        let result = sort.sift(start, sort.count, cpu, mem, api, state, log, sched, limit)?;
        if !matches!(result, GuestCallOutcome::Returned(_)) {
            return Ok(result);
        }
    }
    for end in (1..sort.count).rev() {
        sort.swap(mem, 0, end)?;
        let result = sort.sift(0, end, cpu, mem, api, state, log, sched, limit)?;
        if !matches!(result, GuestCallOutcome::Returned(_)) {
            return Ok(result);
        }
    }
    Ok(GuestCallOutcome::Returned(0))
}
