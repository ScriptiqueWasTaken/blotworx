//! Object/destination pairing in EM1 REL3cbbc0. No entity allocation here.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Pair {
    pub object: usize,
    /// None selects a destination randomly (or slot zero when absent).
    pub destination: Option<usize>,
}

/// None requests native disablement, before incrementing the attempt count.
pub fn pairs(objects: usize, destinations: usize, index: i32) -> Option<Vec<Pair>> {
    if objects == 0 {
        return None;
    }
    if objects > 1 && destinations > 1 && index < 0 {
        if objects != destinations {
            return None;
        }
        return Some(
            (0..objects)
                .map(|object| Pair {
                    object,
                    destination: Some(object),
                })
                .collect(),
        );
    }
    Some(
        (0..objects)
            .filter(|&object| index < 0 || object == index as usize)
            .map(|object| Pair {
                object,
                destination: None,
            })
            .collect(),
    )
}

/// PPC slw uses the low six bits; shifts 32..63 produce zero, not wrapping.
pub fn destination_bit(index: usize) -> u32 {
    1u32.checked_shl((index & 63) as u32).unwrap_or(0)
}

/// REL3cbd08: float selection, reject occupied slots only while capacity
/// exceeds live count, and clamp the RAND_MAX endpoint AFTER rejection.
pub fn random_destination(
    count: usize,
    live: usize,
    occupied: u32,
    mut draw: impl FnMut() -> u32,
) -> usize {
    if count <= 1 {
        return 0;
    }
    let mut index = (count as f32 * (draw() as f32 / 32767.)) as usize;
    if count > live {
        while occupied & destination_bit(index) != 0 {
            index = (count as f32 * (draw() as f32 / 32767.)) as usize;
        }
    }
    index.min(count - 1)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn selection_and_pairing_match_native_branches() {
        assert_eq!(pairs(0, 0, -1), None);
        assert_eq!(pairs(2, 3, -1), None);
        assert_eq!(
            pairs(2, 2, -1),
            Some(vec![
                Pair {
                    object: 0,
                    destination: Some(0)
                },
                Pair {
                    object: 1,
                    destination: Some(1)
                }
            ])
        );
        assert_eq!(
            pairs(2, 0, -1),
            Some(vec![
                Pair {
                    object: 0,
                    destination: None
                },
                Pair {
                    object: 1,
                    destination: None
                }
            ])
        );
        assert_eq!(
            pairs(2, 3, 1),
            Some(vec![Pair {
                object: 1,
                destination: None
            }])
        );
        assert_eq!(pairs(2, 0, 99), Some(vec![]));
    }
    #[test]
    fn random_consumption_rejection_and_endpoint() {
        let mut draws = [0, 16384].into_iter();
        assert_eq!(random_destination(2, 1, 1, || draws.next().unwrap()), 1);
        assert_eq!(draws.next(), None);
        assert_eq!(random_destination(2, 2, 3, || 0), 0);
        assert_eq!(random_destination(2, 1, 2, || 32767), 1);
        assert_eq!(random_destination(1, 0, 0, || panic!("must not draw")), 0);
        assert_eq!(destination_bit(31), 0x80000000);
        assert_eq!(destination_bit(32), 0);
        assert_eq!(destination_bit(64), 1);
    }
}
