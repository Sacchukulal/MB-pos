//! The backoff, and where the count comes from.

use std::time::Duration;

/// Four wrong PINs cost nothing.
pub const LOCKOUT_FREE_ATTEMPTS: u32 = 4;

/// How long this person must wait, given how many times they have failed since they last got
/// in.
#[must_use]
pub fn lockout_after(failures: u32) -> Option<Duration> {
    match failures {
        0..=LOCKOUT_FREE_ATTEMPTS => None,
        5 => Some(Duration::from_secs(30)),
        6 => Some(Duration::from_secs(2 * 60)),
        _ => Some(Duration::from_secs(15 * 60)),
    }
}

/// What the lock screen says.
#[must_use]
pub fn wait_message(remaining: Duration) -> String {
    let seconds = remaining.as_secs();
    if seconds <= 1 {
        return "Wrong PIN. Try again in a moment.".to_owned();
    }
    if seconds < 60 {
        return format!("Wrong PIN. Try again in {seconds} seconds.");
    }
    let minutes = seconds.div_ceil(60);
    if minutes == 1 {
        "Wrong PIN. Try again in a minute.".to_owned()
    } else {
        format!("Wrong PIN. Try again in {minutes} minutes.")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_first_four_mistakes_are_free() {
        for failures in 0..=4 {
            assert_eq!(lockout_after(failures), None, "{failures} failures");
        }
    }

    #[test]
    fn the_wait_grows() {
        let five = lockout_after(5).expect("locked");
        let six = lockout_after(6).expect("locked");
        let seven = lockout_after(7).expect("locked");
        assert!(five < six && six < seven);
        assert_eq!(
            Some(seven),
            lockout_after(70),
            "it stops growing, it does not stop"
        );
    }

    #[test]
    #[allow(
        clippy::integer_division,
        reason = "counting years: the remainder is months, and months do not change the answer"
    )]
    fn a_million_guesses_is_not_worth_starting() {
        // The point of the numbers, as arithmetic rather than as a feeling.
        let per_guess = lockout_after(7).expect("locked").as_secs();
        let years = (1_000_000 * per_guess) / (365 * 24 * 60 * 60);
        assert!(years > 20, "{years} years is not long enough");
    }

    #[test]
    fn the_message_gives_nothing_away() {
        for failures in 5..9 {
            let message = wait_message(lockout_after(failures).expect("locked"));
            // The number in "15 minutes" is fine.
            let lower = message.to_lowercase();
            assert!(
                !lower.contains("attempt"),
                "{message} counts attempts out loud"
            );
            assert!(
                !lower.contains(" of "),
                "{message} counts attempts out loud"
            );
            assert!(
                !lower.contains("last"),
                "{message} warns that this is the last one"
            );
            assert!(message.starts_with("Wrong PIN."));
            assert!(message.contains("Try again"), "{message} says what to do");
        }
    }

    #[test]
    fn the_message_reads_like_english_at_every_step() {
        assert_eq!(
            wait_message(Duration::from_secs(30)),
            "Wrong PIN. Try again in 30 seconds."
        );
        assert_eq!(
            wait_message(Duration::from_secs(60)),
            "Wrong PIN. Try again in a minute."
        );
        assert_eq!(
            wait_message(Duration::from_secs(15 * 60)),
            "Wrong PIN. Try again in 15 minutes."
        );
        // Mid-countdown: 90 seconds left is "2 minutes", never "1.5".
        assert_eq!(
            wait_message(Duration::from_secs(90)),
            "Wrong PIN. Try again in 2 minutes."
        );
    }
}
