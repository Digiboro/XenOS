//! Тесты для чтения hive файлов
//!
//! Используется реальный SYSTEM hive из reference/SYSTEM

use hive::Hive;
use hive::KeyValueDataType;

/// Загружает тестовый SYSTEM hive
fn load_system_hive() -> Vec<u8> {
    let path = concat!(env!("CARGO_MANIFEST_DIR"), "/../../reference/SYSTEM");
    std::fs::read(path).expect("Failed to read reference/SYSTEM file")
}

#[test]
fn test_open_system_hive() {
    let data = load_system_hive();
    let hive = Hive::new(&data).expect("Failed to open SYSTEM hive");
    let root = hive.root_key_node().expect("Failed to get root key");

    // Проверяем имя корневого ключа
    let name = root.name().expect("Failed to get root key name");
    let name_str = name.to_string_lossy();
    println!("Root key name: {}", name_str);

    // Имя должно быть CMI-CreateHive{...} или просто имя hive
    assert!(!name_str.is_empty());
}

#[test]
fn test_enumerate_root_subkeys() {
    let data = load_system_hive();
    let hive = Hive::new(&data).unwrap();
    let root = hive.root_key_node().unwrap();

    // Перечисляем подключи корня
    if let Some(subkeys_result) = root.subkeys() {
        let subkeys = subkeys_result.expect("Failed to get subkeys iterator");

        let mut count = 0;
        let mut found_control_set = false;
        let mut found_select = false;

        for subkey_result in subkeys {
            let subkey = subkey_result.expect("Failed to read subkey");
            let name = subkey.name().expect("Failed to get subkey name");
            let name_str = name.to_string_lossy();
            println!("Subkey: {}", name_str);

            if name_str.contains("ControlSet") {
                found_control_set = true;
            }
            if name_str == "Select" {
                found_select = true;
            }

            count += 1;
        }

        println!("Total subkeys: {}", count);
        assert!(count > 0, "Should have at least one subkey");
        assert!(found_control_set, "Should have ControlSet subkey");
        assert!(found_select, "Should have Select subkey");
    } else {
        panic!("Root should have subkeys");
    }
}

#[test]
fn test_read_select_key() {
    let data = load_system_hive();
    let hive = Hive::new(&data).unwrap();
    let root = hive.root_key_node().unwrap();

    // Ищем ключ Select
    let select = root.subkey("Select");
    assert!(select.is_some(), "Select key should exist");

    let select_key = select.unwrap().expect("Failed to read Select key");

    // Select должен иметь значения Current, Default, Failed, LastKnownGood
    if let Some(values_result) = select_key.values() {
        let values = values_result.expect("Failed to get values iterator");

        let mut found_current = false;
        let mut found_default = false;

        for value_result in values {
            let value = value_result.expect("Failed to read value");
            let name = value.name().expect("Failed to get value name");
            let name_str = name.to_string_lossy();
            println!("Select value: {} (type: {:?})", name_str, value.data_type());

            if name_str == "Current" {
                found_current = true;
                // Current должен быть DWORD
                assert_eq!(
                    value.data_type().unwrap(),
                    KeyValueDataType::RegDWord,
                    "Current should be DWORD"
                );

                let dword = value.dword_data().expect("Failed to read DWORD");
                println!("Current ControlSet: {}", dword);
                assert!(dword >= 1 && dword <= 4, "Current should be 1-4");
            }
            if name_str == "Default" {
                found_default = true;
            }
        }

        assert!(found_current, "Should have Current value");
        assert!(found_default, "Should have Default value");
    }
}

#[test]
fn test_navigate_control_set() {
    let data = load_system_hive();
    let hive = Hive::new(&data).unwrap();
    let root = hive.root_key_node().unwrap();

    // Навигация: ControlSet001\Control
    let cs001 = root.subkey("ControlSet001");
    assert!(cs001.is_some(), "ControlSet001 should exist");

    let cs001_key = cs001.unwrap().unwrap();
    let control = cs001_key.subkey("Control");
    assert!(
        control.is_some(),
        "Control should exist under ControlSet001"
    );

    let control_key = control.unwrap().unwrap();

    // Перечисляем подключи Control
    if let Some(subkeys_result) = control_key.subkeys() {
        let subkeys = subkeys_result.unwrap();

        let mut count = 0;
        for subkey_result in subkeys {
            let subkey = subkey_result.unwrap();
            let name = subkey.name().unwrap();
            println!("ControlSet001\\Control\\{}", name.to_string_lossy());
            count += 1;
        }

        println!("Total Control subkeys: {}", count);
        assert!(count > 10, "Control should have many subkeys");
    }
}

#[test]
fn test_subpath_navigation() {
    let data = load_system_hive();
    let hive = Hive::new(&data).unwrap();
    let root = hive.root_key_node().unwrap();

    // Используем subpath для навигации
    let result = root.subpath("ControlSet001\\Control\\ServiceGroupOrder");

    if let Some(sgo_result) = result {
        let sgo = sgo_result.expect("Failed to navigate to ServiceGroupOrder");
        let name = sgo.name().unwrap();
        assert_eq!(name.to_string_lossy(), "ServiceGroupOrder");

        // Должно быть значение List
        let list_value = sgo.value("List");
        if let Some(list_result) = list_value {
            let list = list_result.expect("Failed to read List value");
            let data_type = list.data_type().unwrap();
            println!("ServiceGroupOrder\\List type: {:?}", data_type);

            // List обычно REG_MULTI_SZ
            assert_eq!(data_type, KeyValueDataType::RegMultiSZ);
        }
    } else {
        println!("ServiceGroupOrder not found - may be different hive structure");
    }
}

#[test]
fn test_read_string_value() {
    let data = load_system_hive();
    let hive = Hive::new(&data).unwrap();
    let root = hive.root_key_node().unwrap();

    // Ищем любое строковое значение
    // ControlSet001\Control\ComputerName\ComputerName обычно имеет REG_SZ
    if let Some(cn_result) = root.subpath("ControlSet001\\Control\\ComputerName\\ComputerName") {
        let cn = cn_result.unwrap();

        if let Some(values_result) = cn.values() {
            for value_result in values_result.unwrap() {
                let value = value_result.unwrap();
                let name = value.name().unwrap();
                let data_type = value.data_type().unwrap();

                println!(
                    "ComputerName value: {} (type: {:?})",
                    name.to_string_lossy(),
                    data_type
                );

                if data_type == KeyValueDataType::RegSZ
                    || data_type == KeyValueDataType::RegExpandSZ
                {
                    // Пробуем прочитать как строку
                    let value_data = value.data().unwrap();
                    let size = match &value_data {
                        hive::KeyValueData::Small(bytes) => bytes.len(),
                        hive::KeyValueData::Big(slices) => slices.len(),
                    };
                    println!("  Data size: {} bytes", size);
                }
            }
        }
    }
}

#[test]
fn test_read_binary_value() {
    let data = load_system_hive();
    let hive = Hive::new(&data).unwrap();
    let root = hive.root_key_node().unwrap();

    // Ищем бинарное значение
    // ControlSet001\Control\Session Manager\Environment часто имеет бинарные данные
    if let Some(env_result) = root.subpath("ControlSet001\\Control\\Session Manager") {
        let sm = env_result.unwrap();

        if let Some(values_result) = sm.values() {
            for value_result in values_result.unwrap() {
                let value = value_result.unwrap();
                let data_type = value.data_type().unwrap();

                if data_type == KeyValueDataType::RegBinary {
                    let name = value.name().unwrap();
                    let value_data = value.data().unwrap();
                    let size = match &value_data {
                        hive::KeyValueData::Small(bytes) => bytes.len(),
                        hive::KeyValueData::Big(slices) => slices.len(),
                    };
                    println!("Binary value: {} ({} bytes)", name.to_string_lossy(), size);
                    break;
                }
            }
        }
    }
}

#[test]
fn test_case_insensitive_lookup() {
    let data = load_system_hive();
    let hive = Hive::new(&data).unwrap();
    let root = hive.root_key_node().unwrap();

    // Windows registry нечувствителен к регистру
    let lower = root.subkey("select");
    let upper = root.subkey("SELECT");
    let mixed = root.subkey("Select");

    // Все три должны найти один и тот же ключ (или все None если нет)
    let lower_found = lower.is_some();
    let upper_found = upper.is_some();
    let mixed_found = mixed.is_some();

    println!(
        "Case sensitivity test: lower={}, upper={}, mixed={}",
        lower_found, upper_found, mixed_found
    );

    // Все должны быть одинаковыми
    assert_eq!(lower_found, upper_found);
    assert_eq!(upper_found, mixed_found);
}

#[test]
fn test_deep_hierarchy() {
    let data = load_system_hive();
    let hive = Hive::new(&data).unwrap();
    let root = hive.root_key_node().unwrap();

    // Пробуем найти глубокий путь
    let deep_path = "ControlSet001\\Services";

    if let Some(services_result) = root.subpath(deep_path) {
        let services = services_result.unwrap();

        // Подсчитываем количество сервисов
        if let Some(subkeys_result) = services.subkeys() {
            let count = subkeys_result.unwrap().count();
            println!("Number of services: {}", count);
            assert!(count > 50, "Should have many services");
        }
    }
}

#[test]
fn test_hive_version() {
    let data = load_system_hive();
    let hive = Hive::new(&data).unwrap();

    // Проверяем версию hive
    let version = hive.minor_version();
    println!("Hive minor version: {:?}", version);

    // Windows 7+ hives обычно версии 5 или выше
}

#[test]
fn test_non_existent_key() {
    let data = load_system_hive();
    let hive = Hive::new(&data).unwrap();
    let root = hive.root_key_node().unwrap();

    // Несуществующий ключ должен вернуть None
    let non_existent = root.subkey("ThisKeyDoesNotExist12345");
    assert!(
        non_existent.is_none(),
        "Non-existent key should return None"
    );

    let non_existent_path = root.subpath("This\\Path\\Does\\Not\\Exist");
    assert!(
        non_existent_path.is_none(),
        "Non-existent path should return None"
    );
}

#[test]
fn test_non_existent_value() {
    let data = load_system_hive();
    let hive = Hive::new(&data).unwrap();
    let root = hive.root_key_node().unwrap();

    // Несуществующее значение должно вернуть None
    if let Some(select_result) = root.subkey("Select") {
        let select = select_result.unwrap();
        let non_existent = select.value("ThisValueDoesNotExist12345");
        assert!(
            non_existent.is_none(),
            "Non-existent value should return None"
        );
    }
}
