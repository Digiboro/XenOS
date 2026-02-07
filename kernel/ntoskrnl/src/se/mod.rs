//! SE - Security Reference Monitor (Security подсистема)
//!
//! Реализация подсистемы безопасности NT ядра уровня Windows 7 (NT 6.1).
//!
//! # Архитектура
//!
//! SE подсистема включает:
//! - **SRM (Security Reference Monitor)**: центральный компонент, выполняющий проверки доступа
//! - **Токены**: объекты, представляющие security context субъектов (процессов/потоков)
//! - **Security Descriptors**: описывают права доступа к объектам
//! - **Привилегии**: специальные права, выходящие за рамки обычного access control
//! - **Аудит**: логирование событий безопасности
//!
//! # Ключевые компоненты
//!
//! - `SeAccessCheck` - основная функция проверки доступа
//! - `SeCreateAccessState` / `SeDeleteAccessState` - управление состоянием запроса доступа
//! - `SeCaptureSubjectContext` - захват security context текущего потока
//! - `SeAssignSecurity` / `SeDeassignSecurity` - назначение/освобождение SD объекта
//! - Token object type - токены как объекты OB
//!
//! # Источники
//!
//! - ReactOS: ntoskrnl/se/*
//! - WDK: wdm.h, ntifs.h
//! - NT5: ntsec.h, sepriv.h

#![allow(dead_code)]

pub mod sid;
pub mod acl;
pub mod sd;
pub mod priv_;
pub mod token;
pub mod subject;
pub mod access_state;
pub mod accesschk;
pub mod audit;
pub mod init;

// Реэкспорт основных типов и функций
pub use sid::*;
pub use acl::*;
pub use sd::*;
pub use priv_::*;
pub use token::*;
pub use subject::*;
pub use access_state::*;
pub use accesschk::*;
pub use audit::*;
pub use init::*;

