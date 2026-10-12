//! Pure x86-64 large-leaf splitting, shared with the kernel and host tests.
use crate::Error;
const ADDRESS: u64 = 0x000f_ffff_ffff_f000;
/// Split a present 1 GiB (level 3) or 2 MiB (level 2) leaf into 512 children.
/// Preserve leaf rights/cache attributes, including relocating PAT to bit 7 at 4 KiB.
pub fn split_leaf(entry: u64, level: u8, index: usize) -> Result<u64, Error> {
    if !(2..=3).contains(&level) || index >= 512 || entry & 0x81 != 0x81 {
        return Err(Error::InvalidRange);
    }
    let parent_shift = 12 + (level as u32 - 1) * 9;
    let child_shift = parent_shift - 9;
    let base = entry & ADDRESS & !((1u64 << parent_shift) - 1);
    let mut flags = entry & !ADDRESS;
    let pat = entry & (1 << 12) != 0;
    if level == 2 {
        flags &= !(1 << 7);
        if pat {
            flags |= 1 << 7;
        }
    } else if pat {
        flags |= 1 << 12;
    }
    Ok(base + ((index as u64) << child_shift) | flags)
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn split_2m_preserves_addresses_rights_and_pat() {
        for pat in [false, true] {
            let flags = 0x8000_0000_0000_01e3u64 | if pat { 1 << 12 } else { 0 };
            for i in 0..512 {
                let e = split_leaf(0x200000 | flags, 2, i).unwrap();
                assert_eq!(e & ADDRESS, 0x200000 + i as u64 * 4096);
                assert_eq!(e & 0x8000_0000_0000_0163, flags & 0x8000_0000_0000_0163);
                assert_eq!(e & (1 << 7) != 0, pat);
            }
        }
    }
    #[test]
    fn split_1g_can_split_again_without_losing_pat() {
        let flags = 0x8000_0000_0000_1183;
        let last = split_leaf(0x4000_0000 | flags, 3, 511).unwrap();
        assert_eq!(last, 0x7fe0_0000 | flags);
        assert_eq!(
            split_leaf(last, 2, 511).unwrap(),
            0x7fff_f000 | 0x8000_0000_0000_0183
        );
        assert!(split_leaf(0, 2, 0).is_err());
        assert!(split_leaf(0x83, 1, 0).is_err());
        assert!(split_leaf(0x83, 3, 512).is_err());
    }
}
