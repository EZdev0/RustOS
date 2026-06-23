use raw_cpuid::CpuId;
use alloc::string::String;
use x86_64::registers::control::{Cr4, Cr4Flags};
use x86_64::registers::xcontrol::{XCr0, XCr0Flags};

pub fn detect_and_init() -> String {
    let cpuid = CpuId::new();
    let mut features = String::from("CPU Features: ");

    if let Some(info) = cpuid.get_feature_info() {
        if info.has_sse42() {
            features.push_str("[SSE4.2] ");
        }
        if info.has_aesni() {
            features.push_str("[AES-NI] ");
        }
        if info.has_avx() {
            features.push_str("[AVX] ");
            unsafe {
                let mut cr4 = Cr4::read();
                cr4.insert(Cr4Flags::OSXSAVE);
                Cr4::write(cr4);

                let mut xcr0 = XCr0::read();
                xcr0.insert(XCr0Flags::SSE | XCr0Flags::AVX);
                XCr0::write(xcr0);
            }
        }
    }

    if let Some(ext) = cpuid.get_extended_feature_info() {
        if ext.has_avx2() {
            features.push_str("[AVX2] ");
        }
    }

    features
}

#[allow(dead_code)]
pub fn has_avx2() -> bool {
    let cpuid = CpuId::new();
    if let Some(ext) = cpuid.get_extended_feature_info() {
        ext.has_avx2()
    } else {
        false
    }
}

#[allow(dead_code)]
pub fn has_avx512f() -> bool {
    let cpuid = CpuId::new();
    if let Some(ext) = cpuid.get_extended_feature_info() {
        ext.has_avx512f()
    } else {
        false
    }
}
