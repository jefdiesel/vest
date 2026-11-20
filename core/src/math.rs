//! Exact integer maths for linear vesting.

/// Floor of `a * b / c`, exact for the whole range of `a: u128`, `b: u64`,
/// `c: u64` whenever the quotient fits in `u128`.
///
/// Callers must keep `b <= c`, which guarantees the quotient is at most `a`
/// and therefore fits in `u128`. The product `a * b` needs 192 bits, so it is
/// kept as three 64-bit limbs and divided by `c` one limb at a time. No
/// floating point, no intermediate overflow. If `c == 0` the function returns
/// 0; callers validate that their time window is non-empty, so this is only a
/// defensive guard.
pub fn mul_div_floor(a: u128, b: u64, c: u64) -> u128 {
    if a == 0 || b == 0 || c == 0 {
        return 0;
    }

    // Split `a` into two 64-bit halves and multiply each by `b`.
    let a_lo = a as u64;
    let a_hi = (a >> 64) as u64;

    let t0 = (a_lo as u128) * (b as u128); // < 2^128
    let p0 = t0 as u64;
    let carry0 = t0 >> 64; // < 2^64

    let t1 = (a_hi as u128) * (b as u128) + carry0; // < 2^128
    let p1 = t1 as u64;
    let p2 = t1 >> 64; // < 2^64

    // Long division of the 192-bit product [p2, p1, p0] by c. The caller's
    // invariants guarantee the quotient fits in 128 bits, so p2/c is zero.
    let mut rem: u128 = 0;
    let cur = (rem << 64) | p2 as u128;
    let q2 = cur / c as u128;
    debug_assert_eq!(q2, 0, "mul_div_floor quotient does not fit in u128");
    rem = cur % c as u128;

    let cur = (rem << 64) | p1 as u128;
    let q1 = cur / c as u128; // < 2^64
    rem = cur % c as u128;

    let cur = (rem << 64) | p0 as u128;
    let q0 = cur / c as u128; // < 2^64

    (q1 << 64) | q0
}

#[cfg(test)]
mod tests {
    use super::*;
    use num_bigint::BigUint;

    /// Independent reference using arbitrary-precision integers.
    fn reference(a: u128, b: u64, c: u64) -> u128 {
        if c == 0 {
            return 0;
        }
        let n = BigUint::from(a) * BigUint::from(b) / BigUint::from(c);
        let bytes = n.to_bytes_le();
        let mut out = [0u8; 16];
        let take = bytes.len().min(16);
        out[..take].copy_from_slice(&bytes[..take]);
        u128::from_le_bytes(out)
    }

    #[test]
    fn simple_cases() {
        assert_eq!(mul_div_floor(100, 3, 2), 150);
        assert_eq!(mul_div_floor(100, 1, 3), 33);
        assert_eq!(mul_div_floor(u128::MAX, 1, 1), u128::MAX);
        assert_eq!(mul_div_floor(7, 0, 3), 0);
        assert_eq!(mul_div_floor(7, 3, 0), 0);
        // A case where the 128-bit product itself would overflow.
        let a = u128::MAX;
        let b = u64::MAX;
        let c = u64::MAX;
        assert_eq!(mul_div_floor(a, b, c), a);
    }

    proptest::proptest! {
        #[test]
        fn matches_bigint_reference(a: u128, b_raw: u64, c_raw: u64) {
            let c = c_raw | 1; // avoid zero
            let b = b_raw.min(c); // callers keep b <= c
            proptest::prop_assert_eq!(mul_div_floor(a, b, c), reference(a, b, c));
        }

        #[test]
        fn result_never_exceeds_a_when_b_le_c(a: u128, b: u64, c_raw: u64) {
            let c = c_raw.max(1);
            let b = b.min(c);
            proptest::prop_assert!(mul_div_floor(a, b, c) <= a);
        }
    }
}
