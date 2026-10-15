use raw_cpuid::{CpuId, CpuIdReader, CpuIdResult};

#[derive(Clone)]
#[allow(dead_code)]
pub struct MockCpuId {
    ebx: u32,
}

impl CpuIdReader for MockCpuId {
    fn cpuid1(&self, eax: u32) -> CpuIdResult {
        if eax == 0 {
            return CpuIdResult {
                eax: 7,
                ebx: 0,
                ecx: 0,
                edx: 0,
            };
        }
        if eax == 7 {
            return CpuIdResult {
                eax: 0,
                ebx: self.ebx,
                ecx: 0,
                edx: 0,
            };
        }
        CpuIdResult {
            eax: 0,
            ebx: 0,
            ecx: 0,
            edx: 0,
        }
    }
    fn cpuid2(&self, eax: u32, ecx: u32) -> CpuIdResult {
        if eax == 7 && ecx == 0 {
            CpuIdResult {
                eax: 0,
                ebx: self.ebx,
                ecx: 0,
                edx: 0,
            }
        } else {
            CpuIdResult {
                eax: 0,
                ebx: 0,
                ecx: 0,
                edx: 0,
            }
        }
    }
}

// These are identical to the kernel logic
pub fn has_avx2_with_cpuid<R: CpuIdReader>(cpuid: CpuId<R>) -> bool {
    if let Some(ext) = cpuid.get_extended_feature_info() {
        ext.has_avx2()
    } else {
        false
    }
}

pub fn has_avx512f_with_cpuid<R: CpuIdReader>(cpuid: CpuId<R>) -> bool {
    if let Some(ext) = cpuid.get_extended_feature_info() {
        ext.has_avx512f()
    } else {
        false
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_has_avx2_true() {
        let mock = MockCpuId { ebx: 1 << 5 }; // bit 5 is AVX2
        let cpuid = CpuId::with_cpuid_reader(mock);
        assert!(has_avx2_with_cpuid(cpuid));
    }

    #[test]
    fn test_has_avx2_false() {
        let mock = MockCpuId { ebx: 0 };
        let cpuid = CpuId::with_cpuid_reader(mock);
        assert!(!has_avx2_with_cpuid(cpuid));
    }

    #[test]
    fn test_has_avx512f_true() {
        let mock = MockCpuId { ebx: 1 << 16 }; // bit 16 is AVX512F
        let cpuid = CpuId::with_cpuid_reader(mock);
        assert!(has_avx512f_with_cpuid(cpuid));
    }

    #[test]
    fn test_has_avx512f_false() {
        let mock = MockCpuId { ebx: 0 };
        let cpuid = CpuId::with_cpuid_reader(mock);
        assert!(!has_avx512f_with_cpuid(cpuid));
    }
}
