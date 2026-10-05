//! Windows launch policy, tested without native process calls.
use std::io;

pub fn spawn_with_job_policy<T>(
    mut create: impl FnMut(bool) -> io::Result<T>,
    parent_in_job: impl FnOnce() -> io::Result<bool>,
) -> io::Result<T> {
    match create(true) {
        // ERROR_ACCESS_DENIED (5) is retried only inside a job. A successful
        // second call, changing only BREAKAWAY_FROM_JOB, confirms that flag was
        // refused. Permissions/other errors still fail; there is no third try.
        Err(error) if error.raw_os_error() == Some(5) && parent_in_job()? => create(false),
        result => result,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn successful_breakaway_does_not_query_parent() {
        let result = spawn_with_job_policy(
            |breakaway| {
                assert!(breakaway);
                Ok(7)
            },
            || panic!("successful spawn must not query parent"),
        );
        assert_eq!(result.unwrap(), 7);
    }
    #[test]
    fn access_denied_in_job_retries_once_without_breakaway() {
        let mut attempts = Vec::new();
        let result = spawn_with_job_policy(
            |breakaway| {
                attempts.push(breakaway);
                if breakaway {
                    Err(io::Error::from_raw_os_error(5))
                } else {
                    Ok(7)
                }
            },
            || Ok(true),
        );
        assert_eq!(result.unwrap(), 7);
        assert_eq!(attempts, [true, false]);
    }
    #[test]
    fn access_denied_without_job_is_not_retried() {
        let mut attempts = Vec::new();
        let result = spawn_with_job_policy::<()>(
            |breakaway| {
                attempts.push(breakaway);
                Err(io::Error::from_raw_os_error(5))
            },
            || Ok(false),
        );
        assert_eq!(result.unwrap_err().raw_os_error(), Some(5));
        assert_eq!(attempts, [true]);
    }
    #[test]
    fn other_spawn_error_is_not_retried_or_queried() {
        let result = spawn_with_job_policy::<()>(
            |_| Err(io::Error::from_raw_os_error(2)),
            || panic!("unrelated error must not query parent"),
        );
        assert_eq!(result.unwrap_err().raw_os_error(), Some(2));
    }
    #[test]
    fn retry_error_is_returned_without_a_third_attempt() {
        let mut attempts = Vec::new();
        let result = spawn_with_job_policy::<()>(
            |breakaway| {
                attempts.push(breakaway);
                Err(io::Error::from_raw_os_error(if breakaway { 5 } else { 2 }))
            },
            || Ok(true),
        );
        assert_eq!(result.unwrap_err().raw_os_error(), Some(2));
        assert_eq!(attempts, [true, false]);
    }
    #[test]
    fn failed_membership_query_does_not_launch_again() {
        let mut attempts = Vec::new();
        let result = spawn_with_job_policy::<()>(
            |breakaway| {
                attempts.push(breakaway);
                Err(io::Error::from_raw_os_error(5))
            },
            || Err(io::Error::from_raw_os_error(6)),
        );
        assert_eq!(result.unwrap_err().raw_os_error(), Some(6));
        assert_eq!(attempts, [true]);
    }
}
