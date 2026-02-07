//! x86_64 архитектурно-зависимый код
//!
//! Содержит инициализацию CPU, GDT, IDT, TSS, MSR и страничной памяти

pub mod context; // Context switch
pub mod cpu; // Инициализация CPU
pub mod gdt; // GDT
pub mod idt; // IDT/прерывания
pub mod msr; // MSR операции
pub mod pcr; // PCR/PRCB структуры
pub mod tss; // TSS
pub mod xstate; // XSAVE/XRSTOR для FPU/SSE/AVX

// pub mod paging;  // Страничная память (будет добавлен в Mm)
