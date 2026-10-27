//! Exact epoch-fraction arithmetic for NervosDAO's 180-epoch lock period.
//! The arithmetic follows `ckb-system-scripts/c/dao.c`: request inclusion
//! rounds elapsed epochs up, then rounds that count up to a 180-epoch cycle.

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct Epoch {
    pub number: i64,
    pub index: i64,
    pub length: i64,
}

impl Epoch {
    pub fn new(number: i64, index: i64, length: i64) -> Result<Self, String> {
        if number < 0 || length <= 0 || index < 0 || index >= length {
            return Err(format!("invalid DAO epoch {number}+{index}/{length}"));
        }
        Ok(Self {
            number,
            index,
            length,
        })
    }

    pub fn display(self) -> String {
        format!("{}+{}/{}", self.number, self.index, self.length)
    }

    /// Signed difference `self - earlier`, represented as an exact fraction.
    fn difference(self, earlier: Self) -> Result<(i128, i128), String> {
        let denominator = i128::from(self.length)
            .checked_mul(i128::from(earlier.length))
            .ok_or("DAO epoch denominator overflow")?;
        let whole = i128::from(self.number - earlier.number)
            .checked_mul(denominator)
            .ok_or("DAO epoch whole difference overflow")?;
        let own_fraction = i128::from(self.index) * i128::from(earlier.length);
        let earlier_fraction = i128::from(earlier.index) * i128::from(self.length);
        let numerator = whole
            .checked_add(own_fraction)
            .and_then(|value| value.checked_sub(earlier_fraction))
            .ok_or("DAO epoch fraction difference overflow")?;
        Ok((numerator, denominator))
    }

    pub fn at_or_after(self, earlier: Self) -> Result<bool, String> {
        Ok(self.difference(earlier)?.0 >= 0)
    }

    fn plus_cycles(self, cycles: i128) -> Result<Self, String> {
        let number = i128::from(self.number)
            .checked_add(cycles.checked_mul(180).ok_or("DAO cycle overflow")?)
            .ok_or("DAO cycle epoch overflow")?;
        Ok(Self {
            number: i64::try_from(number).map_err(|_| "DAO cycle epoch exceeds i64")?,
            ..self
        })
    }
}

/// The next cycle boundary STRICTLY after `now`. At an exact boundary the
/// current phase-0 deposit remains a deposit and begins its next cycle.
pub(super) fn next_phase0_boundary(deposit: Epoch, now: Epoch) -> Result<Epoch, String> {
    let (elapsed, denominator) = now.difference(deposit)?;
    if elapsed < 0 {
        return Err(format!(
            "DAO deposit epoch {} is after tip epoch {}",
            deposit.display(),
            now.display()
        ));
    }
    let cycle = elapsed / (180 * denominator) + 1;
    deposit.plus_cycles(cycle)
}

/// The minimum phase-2 `since` epoch for a request. The request must have
/// appeared strictly after the deposit; a fractional excess rounds upward.
pub(super) fn phase1_ready_epoch(deposit: Epoch, request: Epoch) -> Result<Epoch, String> {
    let (elapsed, denominator) = request.difference(deposit)?;
    if elapsed <= 0 {
        return Err(format!(
            "DAO request epoch {} is not after deposit epoch {}",
            request.display(),
            deposit.display()
        ));
    }
    let period = 180 * denominator;
    let cycles = elapsed
        .checked_add(period - 1)
        .ok_or("DAO request cycle rounding overflow")?
        / period;
    deposit.plus_cycles(cycles)
}

/// One-based nominal six-epoch bucket for a future boundary, 1..=30.
pub(super) fn nominal_day_bucket(now: Epoch, boundary: Epoch) -> Result<usize, String> {
    let (remaining, denominator) = boundary.difference(now)?;
    if remaining <= 0 || remaining > 180 * denominator {
        return Err(format!(
            "DAO future boundary {} outside next 180 epochs from {}",
            boundary.display(),
            now.display()
        ));
    }
    usize::try_from((remaining + 6 * denominator - 1) / (6 * denominator))
        .map_err(|_| "DAO nominal day bucket overflow".to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ep(n: i64, i: i64, l: i64) -> Epoch {
        Epoch::new(n, i, l).unwrap()
    }

    #[test]
    fn exact_cycle_boundary_starts_the_next_phase0_cycle() {
        let deposit = ep(5, 1, 2);
        assert_eq!(
            next_phase0_boundary(deposit, ep(185, 0, 2)).unwrap(),
            ep(185, 1, 2)
        );
        assert_eq!(
            next_phase0_boundary(deposit, ep(185, 1, 2)).unwrap(),
            ep(365, 1, 2)
        );
        assert_eq!(
            nominal_day_bucket(ep(185, 1, 2), ep(365, 1, 2)).unwrap(),
            30
        );
    }

    #[test]
    fn phase1_rounds_fraction_and_equality_like_dao_c() {
        let deposit = ep(5, 1, 2);
        assert_eq!(
            phase1_ready_epoch(deposit, ep(100, 1, 4)).unwrap(),
            ep(185, 1, 2)
        );
        assert_eq!(
            phase1_ready_epoch(deposit, ep(185, 1, 2)).unwrap(),
            ep(185, 1, 2)
        );
        assert_eq!(
            phase1_ready_epoch(deposit, ep(185, 3, 4)).unwrap(),
            ep(365, 1, 2)
        );
        assert!(ep(185, 2, 4).at_or_after(ep(185, 1, 2)).unwrap());
        assert!(!ep(185, 1, 4).at_or_after(ep(185, 1, 2)).unwrap());
        assert!(phase1_ready_epoch(deposit, ep(5, 1, 2)).is_err());
        assert_eq!(
            phase1_ready_epoch(deposit, ep(5, 3, 4)).unwrap(),
            ep(185, 1, 2)
        );
    }

    #[test]
    fn nominal_bucket_respects_fractional_edges() {
        let now = ep(100, 0, 4);
        assert_eq!(nominal_day_bucket(now, ep(106, 0, 2)).unwrap(), 1);
        assert_eq!(nominal_day_bucket(now, ep(106, 1, 4)).unwrap(), 2);
        assert!(nominal_day_bucket(now, now).is_err());
        assert!(Epoch::new(0, 1, 0).is_err());
    }
}
