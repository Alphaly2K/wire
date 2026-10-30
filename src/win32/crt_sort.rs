//! Bounded heapsort plan; the caller owns guest memory and comparator continuations.
#[derive(Clone, Copy)]
pub(crate) enum Action {
    Compare(u32, u32),
    Swap(u32, u32),
    Done,
}
#[derive(Clone, Copy)]
enum After {
    Build(u32),
    Extract(u32),
}
#[derive(Clone, Copy)]
enum Step {
    Next(After),
    Sift {
        root: u32,
        end: u32,
        after: After,
    },
    Children {
        root: u32,
        end: u32,
        child: u32,
        after: After,
    },
    Root {
        root: u32,
        end: u32,
        child: u32,
        after: After,
    },
    SwapRoot {
        root: u32,
        end: u32,
        child: u32,
        after: After,
    },
    Extract(u32),
    Done,
}
#[derive(Clone)]
pub(crate) struct Sort {
    pub base: u32,
    pub count: u32,
    pub width: u32,
    pub compare: u32,
    step: Step,
}
impl Sort {
    pub fn new(base: u32, count: u32, width: u32, compare: u32) -> Result<Self, &'static str> {
        let empty = count < 2 || width == 0;
        if !empty
            && (count > 1_000_000
                || count
                    .checked_mul(width)
                    .is_none_or(|n| n > 64 * 1024 * 1024)
                || compare == 0)
        {
            return Err("qsort bounds or comparator invalid");
        }
        Ok(Self {
            base,
            count,
            width,
            compare,
            step: if empty {
                Step::Done
            } else {
                Step::Next(After::Build(count / 2))
            },
        })
    }
    pub fn size(&self) -> u32 {
        if self.count < 2 || self.width == 0 {
            0
        } else {
            self.count * self.width
        }
    }
    pub fn pointer(&self, index: u32) -> u32 {
        self.base + index * self.width
    }
    pub fn next(&mut self) -> Action {
        loop {
            self.step = match self.step {
                Step::Next(After::Build(0)) => Step::Next(After::Extract(self.count - 1)),
                Step::Next(After::Build(start)) => Step::Sift {
                    root: start - 1,
                    end: self.count,
                    after: After::Build(start - 1),
                },
                Step::Next(After::Extract(0)) => Step::Done,
                Step::Next(After::Extract(end)) => Step::Extract(end),
                Step::Sift { root, end, after } => {
                    let child = root * 2 + 1;
                    if child >= end {
                        Step::Next(after)
                    } else if child + 1 < end {
                        Step::Children {
                            root,
                            end,
                            child,
                            after,
                        }
                    } else {
                        Step::Root {
                            root,
                            end,
                            child,
                            after,
                        }
                    }
                }
                Step::Children { child, .. } => {
                    return Action::Compare(self.pointer(child), self.pointer(child + 1))
                }
                Step::Root { root, child, .. } => {
                    return Action::Compare(self.pointer(root), self.pointer(child))
                }
                Step::SwapRoot { root, child, .. } => {
                    return Action::Swap(self.pointer(root), self.pointer(child))
                }
                Step::Extract(end) => return Action::Swap(self.base, self.pointer(end)),
                Step::Done => return Action::Done,
            };
        }
    }
    pub fn compared(&mut self, result: i32) {
        self.step = match self.step {
            Step::Children {
                root,
                end,
                child,
                after,
            } => Step::Root {
                root,
                end,
                child: child + u32::from(result < 0),
                after,
            },
            Step::Root {
                root,
                end,
                child,
                after,
            } => {
                if result >= 0 {
                    Step::Next(after)
                } else {
                    Step::SwapRoot {
                        root,
                        end,
                        child,
                        after,
                    }
                }
            }
            _ => unreachable!("heapsort comparator result without a comparison"),
        };
    }
    pub fn swapped(&mut self) {
        self.step = match self.step {
            Step::SwapRoot {
                end, child, after, ..
            } => Step::Sift {
                root: child,
                end,
                after,
            },
            Step::Extract(end) => Step::Sift {
                root: 0,
                end,
                after: After::Extract(end - 1),
            },
            _ => unreachable!("heapsort swap result without a swap"),
        };
    }
}
