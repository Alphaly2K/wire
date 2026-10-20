//! Owned native PCM output; callbacks consume host samples and never guest state.
#[cfg(target_os = "macos")]
extern "C" {
    fn wire_audio_create() -> *mut std::ffi::c_void;
    fn wire_audio_destroy(ctx: *mut std::ffi::c_void);
    fn wire_audio_push(ctx: *mut std::ffi::c_void, samples: *const i16, count: u32) -> u32;
    fn wire_audio_stats(ctx: *mut std::ffi::c_void, values: *mut u64);
}

#[derive(Default)]
pub(crate) struct PcmOutput {
    context: usize,
}

impl PcmOutput {
    pub(crate) fn active(&self) -> bool {
        self.context != 0
    }

    pub(crate) fn open(&mut self) -> bool {
        if self.active() {
            return true;
        }
        #[cfg(target_os = "macos")]
        {
            self.context = unsafe { wire_audio_create() } as usize;
        }
        self.active()
    }

    pub(crate) fn push(&mut self, samples: &[i16]) -> u32 {
        #[cfg(target_os = "macos")]
        if self.active() {
            return unsafe {
                wire_audio_push(
                    self.context as *mut _,
                    samples.as_ptr(),
                    samples.len() as u32,
                )
            };
        }
        let _ = samples;
        0
    }

    pub(crate) fn stats(&self, values: &mut [u64; 4]) {
        #[cfg(target_os = "macos")]
        if self.active() {
            unsafe { wire_audio_stats(self.context as *mut _, values.as_mut_ptr()) };
        }
        let _ = values;
    }

    pub(crate) fn close(&mut self) {
        #[cfg(target_os = "macos")]
        if self.active() {
            unsafe { wire_audio_destroy(self.context as *mut _) };
        }
        self.context = 0;
    }
}

impl Drop for PcmOutput {
    fn drop(&mut self) {
        self.close();
    }
}

#[cfg(all(test, target_os = "macos"))]
mod tests {
    use super::PcmOutput;

    #[test]
    #[ignore = "requires a native AudioQueue output device"]
    fn native_pcm_output_accounts_samples_and_closes_once() {
        let mut output = PcmOutput::default();
        assert!(!output.active());
        assert!(output.open());
        assert!(output.open());
        let accepted = output.push(&[0; 2048]);
        assert_eq!(accepted, 2048);
        let mut values = [u64::MAX; 4];
        output.stats(&mut values);
        assert_eq!(values[0] + values[3], accepted as u64);
        output.close();
        assert!(!output.active());
        output.close();
        assert_eq!(output.push(&[0; 16]), 0);
        let mut closed = [77; 4];
        output.stats(&mut closed);
        assert_eq!(closed, [77; 4]);
    }
}
