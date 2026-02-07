//! Преобразование конфигурации в Cargo features

use super::Config;

impl Config {
    /// Преобразует конфигурацию в features для ntoskrnl
    /// Соответствует features в kernel/ntoskrnl/Cargo.toml
    pub fn kernel_features(&self) -> Vec<String> {
        let mut features = vec![];

        // === Core Features ===
        // default = ["alloc", "aml"]
        if self.get_bool("KERNEL_ALLOC") {
            features.push("alloc".into());
        }
        if self.get_bool("KERNEL_AML") {
            features.push("aml".into());
        }

        // === Debug Features ===
        // /DEBUG и /SOS теперь runtime параметры в limine.conf cmdline
        if self.get_bool("DEBUG_KD_FORCE") {
            features.push("kd-force-present".into());
        }

        // === Test Features ===
        if self.get_bool("TEST_KERNEL") {
            features.push("test-kernel".into());
        }
        if self.get_bool("TEST_PS") {
            features.push("ps-test".into());
        }

        // === Trace Features ===
        if self.get_bool("TRACE_ENABLE") {
            features.push("trace".into());
        }
        if self.get_bool("TRACE_CALLS") {
            features.push("trace-calls".into());
        }
        if self.get_bool("TRACE_SCHED") {
            features.push("trace-sched".into());
        }
        if self.get_bool("TRACE_PS") {
            features.push("trace-ps".into());
        }
        if self.get_bool("TRACE_MM") {
            features.push("trace-mm".into());
        }
        if self.get_bool("TRACE_OB") {
            features.push("trace-ob".into());
        }
        if self.get_bool("TRACE_IO") {
            features.push("trace-io".into());
        }
        if self.get_bool("TRACE_STORAGE") {
            features.push("storage-trace".into());
        }

        features
    }

    /// Features для winload (пока нет конфигурируемых)
    pub fn winload_features(&self) -> Vec<String> {
        vec![]
    }
}

