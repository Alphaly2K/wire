//! Exact scalar SSE arithmetic, isolated from guest/process/API code generation.
//! Concrete Single/Double entries keep APFloat monomorphizations in this crate.
mod fast;
mod state;
pub use state::SimdState;

use iced_x86::Mnemonic;
use rustc_apfloat::ieee::{Double, Single};
use rustc_apfloat::{Float, FloatConvert, Round, StatusAnd};

mod sealed {
    pub trait Sealed {}
    impl Sealed for rustc_apfloat::ieee::Single {}
    impl Sealed for rustc_apfloat::ieee::Double {}
}

/// The two SSE formats. Static calls reach concrete functions in this crate.
pub trait Scalar: Float + sealed::Sealed {
    fn accept(
        state: &mut SimdState,
        result: StatusAnd<Self>,
        flags: u32,
        exception: &mut bool,
    ) -> Result<Self, String>;
    fn to_integer(
        state: &mut SimdState,
        bits: u64,
        width: u32,
        round: Round,
        exception: &mut bool,
    ) -> Result<u64, String>;
    fn arithmetic(
        state: &mut SimdState,
        op: Mnemonic,
        a: u64,
        b: u64,
        exception: &mut bool,
    ) -> Result<u64, String>;
    fn predicate(
        state: &mut SimdState,
        a: u64,
        b: u64,
        predicate: u8,
        exception: &mut bool,
    ) -> Result<bool, String>;
    fn compare(
        state: &mut SimdState,
        op: Mnemonic,
        a: u64,
        b: u64,
        exception: &mut bool,
    ) -> Result<(), String>;
    fn sqrt(
        state: &mut SimdState,
        bits: u64,
        fraction: u32,
        bias: i32,
        exception: &mut bool,
    ) -> Result<u64, String>;
}
pub trait ConvertTo<D: Scalar>: Scalar + FloatConvert<D> {
    fn convert_bits(state: &mut SimdState, bits: u64, exception: &mut bool) -> Result<u64, String>;
}
macro_rules! scalar {
    ($format:ty $(, $fast:path)?) => {
        impl Scalar for $format {
            #[inline(never)]
            fn accept(
                state: &mut SimdState,
                result: StatusAnd<Self>,
                flags: u32,
                exception: &mut bool,
            ) -> Result<Self, String> {
                state.accept(result, flags, exception)
            }
            #[inline(never)]
            fn to_integer(
                state: &mut SimdState,
                bits: u64,
                width: u32,
                round: Round,
                exception: &mut bool,
            ) -> Result<u64, String> {
                state.to_integer::<Self>(bits, width, round, exception)
            }
            #[inline(never)]
            fn arithmetic(
                state: &mut SimdState,
                op: Mnemonic,
                a: u64,
                b: u64,
                exception: &mut bool,
            ) -> Result<u64, String> {
                $(if let Some(result) = $fast(state, op, a, b, exception) {
                    return result;
                })?
                state.arithmetic::<Self>(op, a, b, exception)
            }
            #[inline(never)]
            fn predicate(
                state: &mut SimdState,
                a: u64,
                b: u64,
                predicate: u8,
                exception: &mut bool,
            ) -> Result<bool, String> {
                state.predicate::<Self>(a, b, predicate, exception)
            }
            #[inline(never)]
            fn compare(
                state: &mut SimdState,
                op: Mnemonic,
                a: u64,
                b: u64,
                exception: &mut bool,
            ) -> Result<(), String> {
                state.compare::<Self>(op, a, b, exception)
            }
            #[inline(never)]
            fn sqrt(
                state: &mut SimdState,
                bits: u64,
                fraction: u32,
                bias: i32,
                exception: &mut bool,
            ) -> Result<u64, String> {
                state.sqrt::<Self>(bits, fraction, bias, exception)
            }
        }
    };
}
scalar!(Single, fast::single);
scalar!(Double);

macro_rules! conversion {
    ($from:ty, $to:ty) => {
        impl ConvertTo<$to> for $from {
            #[inline(never)]
            fn convert_bits(
                state: &mut SimdState,
                bits: u64,
                exception: &mut bool,
            ) -> Result<u64, String> {
                state.convert::<Self, $to>(bits, exception)
            }
        }
    };
}
conversion!(Single, Double);
conversion!(Double, Single);

impl SimdState {
    #[inline]
    pub fn simd_round(&self) -> Round {
        self.round()
    }
    #[inline]
    pub fn simd_accept<F: Scalar>(
        &mut self,
        result: StatusAnd<F>,
        flags: u32,
        exception: &mut bool,
    ) -> Result<F, String> {
        F::accept(self, result, flags, exception)
    }
    #[inline]
    pub fn simd_to_integer<F: Scalar>(
        &mut self,
        bits: u64,
        width: u32,
        round: Round,
        exception: &mut bool,
    ) -> Result<u64, String> {
        F::to_integer(self, bits, width, round, exception)
    }
    #[inline]
    pub fn simd_arithmetic<F: Scalar>(
        &mut self,
        op: Mnemonic,
        a: u64,
        b: u64,
        exception: &mut bool,
    ) -> Result<u64, String> {
        F::arithmetic(self, op, a, b, exception)
    }
    #[inline]
    pub fn simd_predicate<F: Scalar>(
        &mut self,
        a: u64,
        b: u64,
        predicate: u8,
        exception: &mut bool,
    ) -> Result<bool, String> {
        F::predicate(self, a, b, predicate, exception)
    }
    #[inline]
    pub fn simd_compare<F: Scalar>(
        &mut self,
        op: Mnemonic,
        a: u64,
        b: u64,
        exception: &mut bool,
    ) -> Result<(), String> {
        F::compare(self, op, a, b, exception)
    }
    #[inline]
    pub fn simd_convert<F: ConvertTo<D>, D: Scalar>(
        &mut self,
        bits: u64,
        exception: &mut bool,
    ) -> Result<u64, String> {
        F::convert_bits(self, bits, exception)
    }
    #[inline]
    pub fn simd_sqrt<F: Scalar>(
        &mut self,
        bits: u64,
        fraction: u32,
        bias: i32,
        exception: &mut bool,
    ) -> Result<u64, String> {
        F::sqrt(self, bits, fraction, bias, exception)
    }
}
