use crate::abi::Status;

#[derive(Clone, Copy)]
#[repr(u8)]
pub(crate) enum Kind {
    Engine = 1,
    Document = 2,
    Job = 3,
    Result = 4,
}

struct Slot<T> {
    generation: u32,
    parent: u64,
    value: Option<T>,
}

/// Tagged, generational slots. Retired generations are never recycled.
pub(crate) struct Registry<T> {
    kind: Kind,
    slots: Vec<Slot<T>>,
    free: Vec<usize>,
}

impl<T> Registry<T> {
    pub(crate) fn new(kind: Kind) -> Self {
        Self { kind, slots: Vec::new(), free: Vec::new() }
    }

    pub(crate) fn insert(&mut self, parent: u64, value: T) -> Result<u64, Status> {
        let index = if let Some(index) = self.free.pop() {
            index
        } else {
            u32::try_from(self.slots.len().saturating_add(1)).map_err(|_| Status::LimitExceeded)?;
            self.slots.try_reserve(1).map_err(|_| Status::LimitExceeded)?;
            self.slots.push(Slot { generation: 1, parent: 0, value: None });
            self.slots.len() - 1
        };
        let slot = &mut self.slots[index];
        slot.parent = parent;
        slot.value = Some(value);
        Ok(((self.kind as u64) << 56) | (u64::from(slot.generation) << 32) | (index as u64 + 1))
    }

    fn index(&self, handle: u64) -> Result<usize, Status> {
        if handle >> 56 != self.kind as u64 {
            return Err(Status::InvalidHandle);
        }
        let index = (handle as u32).checked_sub(1).ok_or(Status::InvalidHandle)? as usize;
        let slot = self.slots.get(index).ok_or(Status::InvalidHandle)?;
        let generation = ((handle >> 32) & 0x00ff_ffff) as u32;
        if generation == 0 || slot.generation != generation || slot.value.is_none() {
            return Err(Status::InvalidHandle);
        }
        Ok(index)
    }

    pub(crate) fn get(&self, handle: u64) -> Result<&T, Status> {
        self.slots[self.index(handle)?].value.as_ref().ok_or(Status::InvalidHandle)
    }

    pub(crate) fn get_for_parent(&self, handle: u64, parent: u64) -> Result<&T, Status> {
        let slot = &self.slots[self.index(handle)?];
        if slot.parent != parent {
            return Err(Status::InvalidHandle);
        }
        slot.value.as_ref().ok_or(Status::InvalidHandle)
    }

    pub(crate) fn remove(&mut self, handle: u64) -> Result<T, Status> {
        let index = self.index(handle)?;
        let slot = &mut self.slots[index];
        let value = slot.value.take().ok_or(Status::InvalidHandle)?;
        slot.parent = 0;
        if slot.generation == 0x00ff_ffff || self.free.try_reserve(1).is_err() {
            slot.generation = 0;
        } else {
            slot.generation += 1;
            self.free.push(index);
        }
        Ok(value)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stale_wrong_kind_and_wrong_parent_handles_cannot_resolve_new_values() {
        let mut handles = Registry::new(Kind::Document);
        let old = handles.insert(41, "old").unwrap();
        assert_eq!(handles.get_for_parent(old, 41).unwrap(), &"old");
        assert!(matches!(handles.get_for_parent(old, 42), Err(Status::InvalidHandle)));
        assert!(matches!(handles.get(old ^ (3u64 << 56)), Err(Status::InvalidHandle)));
        assert_eq!(handles.remove(old).unwrap(), "old");
        let new = handles.insert(41, "new").unwrap();
        assert_ne!(old, new);
        assert!(matches!(handles.get(old), Err(Status::InvalidHandle)));
        assert_eq!(handles.get(new).unwrap(), &"new");
        assert!(matches!(handles.remove(0), Err(Status::InvalidHandle)));
        assert!(matches!(handles.remove(u64::MAX), Err(Status::InvalidHandle)));
    }

    #[test]
    fn generation_exhaustion_retires_the_slot_instead_of_reusing_an_old_identity() {
        let mut handles = Registry::new(Kind::Document);
        handles.insert(9, 17).unwrap();
        handles.slots[0].generation = 0x00ff_ffff;
        let last = 0x02ff_ffff_0000_0001;
        assert_eq!(handles.remove(last).unwrap(), 17);
        let next = handles.insert(9, 23).unwrap();
        assert_ne!(next & 0xffff_ffff, 1);
        assert!(matches!(handles.get(last), Err(Status::InvalidHandle)));
        assert_eq!(*handles.get(next).unwrap(), 23);
    }
}
