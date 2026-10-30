use super::{
    call::Call,
    catalog::MenuApi as M,
    context,
    modules::Failure,
    user::{
        geometry,
        menus::{self, Menus, Windows},
        User, Window,
    },
    ServiceError,
};
use crate::{
    machine::State,
    memory::{Memory, WRITE},
    process::Loaded,
};
use std::collections::BTreeMap;

struct Adapter<'a>(&'a mut Memory);
impl menus::Memory for Adapter<'_> {
    type Error = Failure;
    fn read(&self, p: u32, bytes: &mut [u8]) -> Result<(), Failure> {
        self.0.read(p.into(), bytes).map_err(Into::into)
    }
    fn check_write(&self, p: u32, n: usize) -> Result<(), Failure> {
        self.0.check(p.into(), n, WRITE).map_err(Into::into)
    }
    fn write(&mut self, p: u32, bytes: &[u8]) -> Result<(), Failure> {
        self.0.write(p.into(), bytes).map_err(Into::into)
    }
}
impl Windows for BTreeMap<u64, Window> {
    fn menu(&self, h: u32) -> Result<(u32, u32), u32> {
        self.get(&u64::from(h))
            .map(|w| (w.style, w.menu as u32))
            .ok_or(1400)
    }
    fn set_menu(&mut self, h: u32, menu: u32) -> Result<(), u32> {
        self.get_mut(&u64::from(h)).ok_or(1400u32)?.menu = menu.into();
        Ok(())
    }
    fn retain_menus(&mut self, menus: &Menus) {
        for w in self.values_mut() {
            if w.style & 0x40000000 == 0 && !menus.contains(w.menu as u32) {
                w.menu = 0;
            }
        }
    }
}
struct Change {
    window: u64,
    menu: u64,
    client: [i32; 4],
}
fn default_client(w: &Window, menu: bool) -> [i32; 4] {
    let f = geometry::frame(w.style, w.ex_style, menu, true);
    let left = w.rect[0].saturating_add(f[0]);
    let top = w.rect[1].saturating_add(f[1]);
    [
        left,
        top,
        w.rect[2].saturating_sub(f[2]).max(left),
        w.rect[3].saturating_sub(f[3]).max(top),
    ]
}
fn client(w: &Window, menu: u64) -> [i32; 4] {
    let old = w.style & 0x40000000 == 0 && w.menu != 0;
    let new = w.style & 0x40000000 == 0 && menu != 0;
    if old == new {
        return w.client;
    }
    default_client(w, new)
}
fn changes(id: M, a: &[u32; 16], user: &User) -> Result<Vec<Change>, u32> {
    let mut changes = Vec::new();
    if id == M::Set {
        let (style, _) = user.windows.menu(a[0])?;
        if style & (0x40000000 | 0x80000000) == 0x40000000 {
            return Err(1436);
        }
        if a[1] != 0 && !user.menus.contains(a[1]) {
            return Err(1401);
        }
        let window = u64::from(a[0]);
        let w = &user.windows[&window];
        changes.push(Change {
            window,
            menu: a[1].into(),
            client: client(w, a[1].into()),
        });
    } else if matches!(id, M::Destroy | M::Delete) {
        let retired = user.menus.retirements(id, a)?;
        if !retired.is_empty() {
            for (&h, w) in &user.windows {
                if w.style & 0x40000000 == 0
                    && w.menu != 0
                    && (retired.contains(&(w.menu as u32)) || !user.menus.contains(w.menu as u32))
                {
                    changes.push(Change {
                        window: h,
                        menu: 0,
                        client: client(w, 0),
                    });
                }
            }
        }
    }
    for c in &changes {
        if !(0..=8192).contains(&(i64::from(c.client[2]) - i64::from(c.client[0])))
            || !(0..=8192).contains(&(i64::from(c.client[3]) - i64::from(c.client[1])))
        {
            return Err(87);
        }
    }
    Ok(changes)
}
fn requests(changes: &[Change]) -> Vec<(u64, u32, u32)> {
    changes
        .iter()
        .map(|c| {
            (
                c.window,
                (c.client[2] - c.client[0]) as u32,
                (c.client[3] - c.client[1]) as u32,
            )
        })
        .collect()
}
fn commit(user: &mut User, changes: Vec<Change>) {
    for c in changes {
        let w = user.windows.get_mut(&c.window).unwrap();
        let changed = w.client != c.client;
        w.menu = c.menu;
        w.client = c.client;
        if changed {
            user.validate(c.window, None);
            let _ = user.invalidate(c.window, None, true);
        }
    }
}
pub(super) fn retire_windows(user: &mut User) {
    let changes: Vec<_> = user
        .windows
        .iter()
        .filter(|(_, w)| {
            w.style & 0x40000000 == 0 && w.menu != 0 && !user.menus.contains(w.menu as u32)
        })
        .map(|(&h, w)| Change {
            window: h,
            menu: 0,
            client: client(w, 0),
        })
        .collect();
    // Window retirement cannot fail. If enlargement is exhausted, keep the
    // existing DCs/pixels; a later GetDC retries allocation for the new geometry.
    if let Ok(prepared) = user.gdi.prepare_resizes(&requests(&changes)) {
        user.gdi.commit_resizes(prepared);
    }
    commit(user, changes);
}
pub(super) fn dispatch(
    id: M,
    call: &Call,
    state: &State,
    memory: &mut Memory,
    image: Option<&mut Loaded>,
) -> Result<u64, ServiceError> {
    let image = image.ok_or(ServiceError::Unavailable("menus require a loaded image"))?;
    let a = call.arguments.map(|v| v as u32);
    let result = (|| -> Result<u32, Failure> {
        let changes = changes(id, &a, &image.user)?;
        let prepared = image.user.gdi.prepare_resizes(&requests(&changes))?;
        let value =
            image
                .user
                .menus
                .apply(id, &a, &mut Adapter(memory), &mut image.user.windows)?;
        image.user.gdi.commit_resizes(prepared);
        commit(&mut image.user, changes);
        Ok(value)
    })();
    match result {
        Ok(value) => Ok(value.into()),
        Err(Failure::Fault(fault)) => Err(fault.into()),
        Err(Failure::Windows(code)) => {
            memory.write_int(context::error_address(state, memory.mode()), 4, code.into())?;
            Ok(if id.minus_one_on_error() {
                u32::MAX.into()
            } else {
                0
            })
        }
    }
}
