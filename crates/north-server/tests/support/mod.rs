use std::{
    env,
    ffi::OsString,
    sync::{Mutex, MutexGuard, OnceLock},
};

static ENVIRONMENT_LOCK: OnceLock<Mutex<()>> = OnceLock::new();

#[allow(dead_code)]
pub fn test_otp_key() -> north_persistence::OtpKey {
    north_persistence::OtpKey::from_hex(
        "000102030405060708090a0b0c0d0e0f101112131415161718191a1b1c1d1e1f",
    )
    .expect("valid test OTP key")
}

pub struct ScopedEnvVar {
    name: &'static str,
    previous: Option<OsString>,
    _guard: MutexGuard<'static, ()>,
}

impl ScopedEnvVar {
    pub fn set(name: &'static str, value: &str) -> Self {
        let guard = ENVIRONMENT_LOCK
            .get_or_init(|| Mutex::new(()))
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let previous = env::var_os(name);
        env::set_var(name, value);
        Self {
            name,
            previous,
            _guard: guard,
        }
    }
}

impl Drop for ScopedEnvVar {
    fn drop(&mut self) {
        match self.previous.as_deref() {
            Some(value) => env::set_var(self.name, value),
            None => env::remove_var(self.name),
        }
    }
}
