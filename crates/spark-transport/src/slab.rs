use crate::TaskToken;

/// Generational slab.
///
/// - Each slot keeps a monotonically increasing generation counter.
/// - `TaskToken { idx, gen }` is valid iff `slots[idx].gen == gen` AND the slot is occupied.
#[derive(Debug)]
pub struct Slab<T> {
    slots: Vec<Slot<T>>,
    free: Vec<u32>,
}

#[derive(Debug)]
struct Slot<T> {
    gen: u32,
    value: Option<T>,
}

impl<T> Slab<T> {
    pub fn new(capacity: usize) -> Self {
        let mut slots = Vec::with_capacity(capacity);
        for _ in 0..capacity {
            slots.push(Slot { gen: 1, value: None });
        }
        let free = (0..capacity as u32).rev().collect();
        Self { slots, free }
    }

    pub fn insert(&mut self, value: T) -> Option<TaskToken> {
        let idx = self.free.pop()?;
        let slot = self.slots.get_mut(idx as usize)?;
        slot.value = Some(value);
        Some(TaskToken { idx, gen: slot.gen })
    }

    pub fn get_mut(&mut self, token: TaskToken) -> Option<&mut T> {
        let slot = self.slots.get_mut(token.idx as usize)?;
        if slot.gen != token.gen {
            return None;
        }
        slot.value.as_mut()
    }

    /// Find a live token whose value matches a predicate.
    ///
    /// This is intended for rare slow-path cleanup (e.g. channel teardown) where
    /// keeping an extra index/map would add maintenance burden.
    pub fn find_token_by<F>(&self, mut f: F) -> Option<TaskToken>
    where
        F: FnMut(&T) -> bool,
    {
        for (idx, slot) in self.slots.iter().enumerate() {
            let Some(v) = slot.value.as_ref() else {
                continue;
            };
            if f(v) {
                return Some(TaskToken {
                    idx: idx as u32,
                    gen: slot.gen,
                });
            }
        }
        None
    }

    /// Temporarily take the value out of a slot.
    ///
    /// This is useful when the caller needs to re-enter `&mut self` methods
    /// while manipulating a slot value (e.g. driver state machines).
    ///
    /// Note:
    /// - The slot remains reserved (its index is NOT returned to the free list).
    /// - The caller must either [`put`](Self::put) the value back, or [`free`](Self::free)
    ///   the token.
    pub fn take(&mut self, token: TaskToken) -> Option<T> {
        let slot = self.slots.get_mut(token.idx as usize)?;
        if slot.gen != token.gen {
            return None;
        }
        slot.value.take()
    }

    /// Put a value back into a previously `take`n slot.
    ///
    /// Returns `true` if the value is successfully stored.
    pub fn put(&mut self, token: TaskToken, value: T) -> bool {
        let Some(slot) = self.slots.get_mut(token.idx as usize) else {
            return false;
        };
        if slot.gen != token.gen {
            return false;
        }
        if slot.value.is_some() {
            return false;
        }
        slot.value = Some(value);
        true
    }

    pub fn free(&mut self, token: TaskToken) {
        if let Some(slot) = self.slots.get_mut(token.idx as usize) {
            // Invalidate stale tokens and release value.
            slot.gen = slot.gen.wrapping_add(1).max(1);
            slot.value = None;
            self.free.push(token.idx);
        }
    }
}
