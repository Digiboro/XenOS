//! Virtual to Physical Address Translation
//!
//! Полный page table walk для получения физического адреса.

use super::pte::*;

/// Конвертирует виртуальный адрес в физический через page table walk
///
/// # Arguments
/// * `virtual_address` - виртуальный адрес
///
/// # Returns
/// Some(physical_address) или None если страница не замаппирована
pub unsafe fn mm_virtual_to_physical(virtual_address: u64) -> Option<u64> {
    // Получаем PXE (PML4E)
    let pxe_ptr = mm_get_pxe_address(virtual_address);
    let pxe = core::ptr::read_volatile(pxe_ptr as *const u64);
    
    if (pxe & PTE_VALID) == 0 {
        return None; // PML4E не present
    }
    
    // Получаем PPE (PDPTE)
    let ppe_ptr = mm_get_ppe_address(virtual_address);
    let ppe = core::ptr::read_volatile(ppe_ptr as *const u64);
    
    if (ppe & PTE_VALID) == 0 {
        return None; // PDPTE не present
    }
    
    // Проверяем 1GB page
    if (ppe & (1 << 7)) != 0 {
        // 1GB large page
        let phys_page = ppe & 0x000F_FFFF_C000_0000u64;
        let offset = virtual_address & 0x3FFF_FFFF;
        return Some(phys_page | offset);
    }
    
    // Получаем PDE
    let pde_ptr = mm_get_pde_address(virtual_address);
    let pde = core::ptr::read_volatile(pde_ptr as *const u64);
    
    if (pde & PTE_VALID) == 0 {
        return None; // PDE не present
    }
    
    // Проверяем 2MB page
    if (pde & (1 << 7)) != 0 {
        // 2MB large page
        let phys_page = pde & 0x000F_FFFF_FFE0_0000u64;
        let offset = virtual_address & 0x1F_FFFF;
        return Some(phys_page | offset);
    }
    
    // Получаем PTE
    let pte_ptr = mm_get_pte_address(virtual_address);
    let pte = core::ptr::read_volatile(pte_ptr as *const u64);
    
    if (pte & PTE_VALID) == 0 {
        return None; // PTE не present
    }
    
    // 4KB page
    let phys_page = pte & 0x000F_FFFF_FFFF_F000u64;
    let offset = virtual_address & 0xFFF;
    Some(phys_page | offset)
}

