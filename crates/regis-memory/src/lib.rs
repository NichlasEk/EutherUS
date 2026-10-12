#![no_std]
//! Bounded physical-frame bookkeeping. No pointers are dereferenced by this crate.
pub const PAGE: u64 = 4096;
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Error {
    InvalidMap,
    Overlap,
    Overflow,
    InvalidRange,
    Busy,
    InvalidFree,
    Exhausted,
}
#[derive(Debug, Clone, Copy)]
pub struct Region {
    pub kind: u32,
    pub start: u64,
    pub end: u64,
    pub attributes: u64,
}
impl Region {
    pub fn conventional(self) -> bool {
        self.kind == 7 && self.attributes & ((1 << 63) | (1 << 15) | (1 << 18)) == 0
    }
}
pub struct Map<'a> {
    bytes: &'a [u8],
    stride: usize,
}
impl<'a> Map<'a> {
    pub fn parse(bytes: &'a [u8], stride: usize, version: u32) -> Result<Self, Error> {
        if version != 1
            || stride < 40
            || stride % 8 != 0
            || bytes.is_empty()
            || bytes.len() % stride != 0
            || bytes.len() / stride > 4096
        {
            return Err(Error::InvalidMap);
        }
        let map = Self { bytes, stride };
        for i in 0..map.len() {
            let a = map.region(i)?;
            for j in 0..i {
                let b = map.region(j)?;
                if a.start < b.end && b.start < a.end {
                    return Err(Error::Overlap);
                }
            }
        }
        Ok(map)
    }
    pub fn len(&self) -> usize {
        self.bytes.len() / self.stride
    }
    pub fn is_empty(&self) -> bool {
        self.bytes.is_empty()
    }
    pub fn region(&self, i: usize) -> Result<Region, Error> {
        if i >= self.len() {
            return Err(Error::InvalidRange);
        }
        let b = &self.bytes[i * self.stride..][..40];
        let kind = u32::from_le_bytes(b[..4].try_into().unwrap());
        let start = u64::from_le_bytes(b[8..16].try_into().unwrap());
        let pages = u64::from_le_bytes(b[24..32].try_into().unwrap());
        let attributes = u64::from_le_bytes(b[32..40].try_into().unwrap());
        if start % PAGE != 0 || pages == 0 {
            return Err(Error::InvalidMap);
        }
        let end = start
            .checked_add(pages.checked_mul(PAGE).ok_or(Error::Overflow)?)
            .ok_or(Error::Overflow)?;
        Ok(Region {
            kind,
            start,
            end,
            attributes,
        })
    }
    pub fn containing(&self, start: u64, bytes: u64) -> Option<Region> {
        let end = start.checked_add(bytes)?;
        if bytes == 0 {
            return None;
        }
        (0..self.len())
            .filter_map(|i| self.region(i).ok())
            .find(|r| r.start <= start && end <= r.end)
    }
}
// Reserved, free, first, interior, last, single. Boundaries reject partial frees.
const RESERVED: u8 = 0;
const FREE: u8 = 1;
const FIRST: u8 = 2;
const BODY: u8 = 3;
const LAST: u8 = 4;
const SINGLE: u8 = 5;
pub struct Frames<'a> {
    states: &'a mut [u8],
    free: usize,
    cursor: usize,
}
impl<'a> Frames<'a> {
    pub fn from_map(states: &'a mut [u8], map: &Map<'_>) -> Self {
        states.fill(RESERVED);
        let mut this = Self {
            states,
            free: 0,
            cursor: 0,
        };
        for i in 0..map.len() {
            let r = map.region(i).unwrap();
            if !r.conventional() {
                continue;
            }
            let start = (r.start / PAGE).min(this.states.len() as u64) as usize;
            let end = (r.end / PAGE).min(this.states.len() as u64) as usize;
            this.states[start..end].fill(FREE);
            this.free += end - start;
        }
        this
    }
    pub fn free_pages(&self) -> usize {
        self.free
    }
    pub fn capacity(&self) -> usize {
        self.states.len()
    }
    /// Reserve every intersecting page, including unaligned edges. Transactional.
    pub fn reserve(&mut self, start: u64, bytes: u64) -> Result<(), Error> {
        if bytes == 0 {
            return Err(Error::InvalidRange);
        }
        let end = start.checked_add(bytes).ok_or(Error::Overflow)?;
        let a = (start / PAGE).min(self.states.len() as u64) as usize;
        let b = (end / PAGE + u64::from(end % PAGE != 0)).min(self.states.len() as u64) as usize;
        if self.states[a..b].iter().any(|s| *s > FREE) {
            return Err(Error::Busy);
        }
        for s in &mut self.states[a..b] {
            if *s == FREE {
                self.free -= 1;
            }
            *s = RESERVED;
        }
        Ok(())
    }
    pub fn is_reserved(&self, address: u64) -> bool {
        self.states
            .get((address / PAGE) as usize)
            .is_none_or(|s| *s == RESERVED)
    }
    pub fn allocate(&mut self, pages: usize) -> Result<u64, Error> {
        if pages == 0 {
            return Err(Error::InvalidRange);
        }
        if pages > self.free {
            return Err(Error::Exhausted);
        }
        // Search complete linear intervals; a run never wraps across physical end.
        let mut found = None;
        for (start, end) in [(self.cursor, self.states.len()), (0, self.cursor)] {
            let mut run = 0;
            for i in start..end {
                run = if self.states[i] == FREE { run + 1 } else { 0 };
                if run == pages {
                    found = Some(i + 1 - pages);
                    break;
                }
            }
            if found.is_some() {
                break;
            }
        }
        // A free run can cross the search cursor after a prior release.
        if found.is_none() && self.cursor > 0 {
            let mut run = 0;
            for (i, s) in self.states.iter().enumerate() {
                run = if *s == FREE { run + 1 } else { 0 };
                if run == pages {
                    found = Some(i + 1 - pages);
                    break;
                }
            }
        }
        let start = found.ok_or(Error::Exhausted)?;
        self.states[start..start + pages].fill(BODY);
        self.states[start] = if pages == 1 { SINGLE } else { FIRST };
        if pages > 1 {
            self.states[start + pages - 1] = LAST;
        }
        self.free -= pages;
        self.cursor = (start + pages) % self.states.len();
        Ok(start as u64 * PAGE)
    }
    /// Low-level address/length API: caller must own the entire live allocation.
    /// Rejects double/partial/misaligned frees. Does not protect against stale ABA addresses.
    pub fn release(&mut self, address: u64, pages: usize) -> Result<(), Error> {
        if address % PAGE != 0 || pages == 0 {
            return Err(Error::InvalidFree);
        }
        let start = usize::try_from(address / PAGE).map_err(|_| Error::InvalidFree)?;
        let end = start.checked_add(pages).ok_or(Error::InvalidFree)?;
        let run = self.states.get(start..end).ok_or(Error::InvalidFree)?;
        let valid = if pages == 1 {
            run[0] == SINGLE
        } else {
            run[0] == FIRST
                && run[pages - 1] == LAST
                && run[1..pages - 1].iter().all(|s| *s == BODY)
        };
        if !valid {
            return Err(Error::InvalidFree);
        }
        self.states[start..end].fill(FREE);
        self.free += pages;
        self.cursor = self.cursor.min(start);
        Ok(())
    }
}
#[cfg(test)]
mod tests {
    extern crate std;
    use super::*;
    use std::vec::Vec;
    fn bytes(regions: &[(u32, u64, u64, u64)], stride: usize) -> Vec<u8> {
        let mut data = std::vec![0;regions.len()*stride];
        for (i, (kind, start, pages, attr)) in regions.iter().enumerate() {
            let b = &mut data[i * stride..];
            b[..4].copy_from_slice(&kind.to_le_bytes());
            b[8..16].copy_from_slice(&start.to_le_bytes());
            b[24..32].copy_from_slice(&pages.to_le_bytes());
            b[32..40].copy_from_slice(&attr.to_le_bytes());
        }
        data
    }
    #[test]
    fn stride_types_attributes_and_capacity_are_respected() {
        let raw = bytes(
            &[
                (7, 0, 10, 0),
                (2, 10 * PAGE, 2, 0),
                (7, 12 * PAGE, 2, 1 << 63),
                (7, 14 * PAGE, 4, 0),
            ],
            48,
        );
        let map = Map::parse(&raw, 48, 1).unwrap();
        let mut state = [0; 16];
        let mut a = Frames::from_map(&mut state, &map);
        assert_eq!(a.free_pages(), 12);
        a.reserve(0, PAGE).unwrap();
        while let Ok(p) = a.allocate(1) {
            assert!(p >= PAGE && !(10 * PAGE..14 * PAGE).contains(&p) && p < 16 * PAGE);
        }
    }
    #[test]
    fn malformed_and_overlapping_maps_fail_closed() {
        for (raw, stride, version) in [
            (bytes(&[(7, 0, 2, 0), (2, PAGE, 2, 0)], 48), 48, 1),
            (bytes(&[(7, 1, 1, 0)], 48), 48, 1),
            (bytes(&[(7, 0, 0, 0)], 48), 48, 1),
            (bytes(&[(7, 0, u64::MAX, 0)], 48), 48, 1),
            (bytes(&[(7, 0, 1, 0)], 48), 40, 1),
            (bytes(&[(7, 0, 1, 0)], 48), 48, 2),
        ] {
            assert!(Map::parse(&raw, stride, version).is_err());
        }
    }
    #[test]
    fn reservations_and_invalid_frees_are_transactional() {
        let raw = bytes(&[(7, 0, 32, 0)], 40);
        let map = Map::parse(&raw, 40, 1).unwrap();
        let mut state = [0; 32];
        let mut a = Frames::from_map(&mut state, &map);
        a.reserve(PAGE + 1, PAGE).unwrap();
        assert!(a.is_reserved(PAGE) && a.is_reserved(2 * PAGE));
        let p = a.allocate(4).unwrap();
        let before = a.free_pages();
        for (addr, n) in [(p + PAGE, 3), (p, 3), (p, 5), (p + 1, 4), (u64::MAX, 1)] {
            assert_eq!(a.release(addr, n), Err(Error::InvalidFree));
            assert_eq!(a.free_pages(), before);
        }
        assert_eq!(a.reserve(p - PAGE, 2 * PAGE), Err(Error::Busy));
        assert_eq!(a.free_pages(), before);
        a.release(p, 4).unwrap();
        assert_eq!(a.release(p, 4), Err(Error::InvalidFree));
        assert_eq!(a.free_pages(), before + 4);
    }
    #[test]
    fn exhaustion_fragmentation_and_coalescence() {
        let raw = bytes(&[(7, 0, 64, 0)], 48);
        let map = Map::parse(&raw, 48, 1).unwrap();
        let mut state = [0; 64];
        let mut a = Frames::from_map(&mut state, &map);
        let mut live = [0; 64];
        for p in &mut live {
            *p = a.allocate(1).unwrap();
        }
        assert_eq!(a.allocate(1), Err(Error::Exhausted));
        for i in (0..64).step_by(2) {
            a.release(live[i], 1).unwrap();
        }
        assert_eq!(a.allocate(2), Err(Error::Exhausted));
        for i in (1..64).step_by(2) {
            a.release(live[i], 1).unwrap();
        }
        assert_eq!(a.allocate(64), Ok(0));
        a.release(0, 64).unwrap();
        assert_eq!(a.free_pages(), 64);
    }
    #[test]
    fn mixed_workload_preserves_disjoint_live_allocations() {
        let raw = bytes(&[(7, 0, 128, 0)], 48);
        let map = Map::parse(&raw, 48, 1).unwrap();
        let mut state = [0; 128];
        let mut a = Frames::from_map(&mut state, &map);
        a.reserve(0, 3 * PAGE).unwrap();
        let mut live = Vec::new();
        let mut random = 17u64;
        for _ in 0..5000 {
            random = random.wrapping_mul(6364136223846793005).wrapping_add(1);
            if random & 3 == 0 && !live.is_empty() {
                let (p, n) = live.swap_remove((random as usize >> 8) % live.len());
                a.release(p, n).unwrap();
            } else {
                let n = ((random >> 8) % 9 + 1) as usize;
                if let Ok(p) = a.allocate(n) {
                    assert!(p >= 3 * PAGE);
                    for &(q, m) in &live {
                        assert!(p + n as u64 * PAGE <= q || q + m as u64 * PAGE <= p);
                    }
                    live.push((p, n));
                }
            }
            assert_eq!(
                a.free_pages() + live.iter().map(|(_, n)| n).sum::<usize>(),
                125
            );
        }
    }
}
