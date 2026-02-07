//! Тесты для функционала записи hive
//!
//! Эти тесты запускаются в std окружении.

#![cfg(feature = "write")]

use hive::Hive;
use hive::HiveBuilder;
use hive::KeyValueDataType;

#[test]
fn test_create_empty_hive() {
    let builder = HiveBuilder::new("TEST").unwrap();
    let data = builder.build().unwrap();

    // Проверяем сигнатуру
    assert_eq!(&data[0..4], b"regf");

    // Проверяем что hive валиден
    let hive = Hive::new(&data).unwrap();
    let root = hive.root_key_node().unwrap();

    // Корневой ключ должен называться "TEST"
    let name = root.name().unwrap();
    assert!(name.to_string_lossy().contains("TEST"));
}

#[test]
fn test_create_subkey() {
    let mut builder = HiveBuilder::new("ROOT").unwrap();
    let root = builder.root_key();

    let subkey = builder.create_subkey(&root, "SubKey1").unwrap();
    let _ = builder.create_subkey(&subkey, "Nested").unwrap();

    let data = builder.build().unwrap();

    // Проверяем что можем прочитать созданные ключи
    let hive = Hive::new(&data).unwrap();
    let root = hive.root_key_node().unwrap();

    // Ищем SubKey1 - subkey() возвращает Option<Result<KeyNode>>
    let found = root.subkey("SubKey1");
    assert!(found.is_some(), "SubKey1 should exist");

    let subkey1 = found.unwrap().unwrap();
    let nested = subkey1.subkey("Nested");
    assert!(nested.is_some(), "Nested should exist");
}

#[test]
fn test_set_dword_value() {
    let mut builder = HiveBuilder::new("TEST").unwrap();
    let root = builder.root_key();

    builder.set_dword(&root, "TestValue", 0x12345678).unwrap();

    let data = builder.build().unwrap();

    // Проверяем чтение значения
    let hive = Hive::new(&data).unwrap();
    let root = hive.root_key_node().unwrap();

    // value() возвращает Option<Result<KeyValue>>
    let value = root.value("TestValue");
    assert!(value.is_some(), "TestValue should exist");

    let val = value.unwrap().unwrap();
    let dword = val.dword_data().unwrap();
    assert_eq!(dword, 0x12345678);
}

#[test]
fn test_set_qword_value() {
    let mut builder = HiveBuilder::new("TEST").unwrap();
    let root = builder.root_key();

    builder
        .set_qword(&root, "BigValue", 0x123456789ABCDEF0)
        .unwrap();

    let data = builder.build().unwrap();

    let hive = Hive::new(&data).unwrap();
    let root = hive.root_key_node().unwrap();

    let value = root.value("BigValue");
    assert!(value.is_some(), "BigValue should exist");

    let val = value.unwrap().unwrap();
    let qword = val.qword_data().unwrap();
    assert_eq!(qword, 0x123456789ABCDEF0);
}

#[test]
fn test_set_string_value() {
    let mut builder = HiveBuilder::new("TEST").unwrap();
    let root = builder.root_key();

    builder
        .set_string(&root, "StringVal", r"C:\Windows\System32")
        .unwrap();

    let data = builder.build().unwrap();

    let hive = Hive::new(&data).unwrap();
    let root = hive.root_key_node().unwrap();

    let value = root.value("StringVal");
    assert!(value.is_some(), "StringVal should exist");

    // Проверяем тип данных
    let val = value.unwrap().unwrap();
    assert_eq!(val.data_type().unwrap(), KeyValueDataType::RegSZ);
}

#[test]
fn test_set_binary_value() {
    let mut builder = HiveBuilder::new("TEST").unwrap();
    let root = builder.root_key();

    let binary_data = [0xDE, 0xAD, 0xBE, 0xEF, 0x00, 0x11, 0x22, 0x33];
    builder.set_binary(&root, "BinData", &binary_data).unwrap();

    let data = builder.build().unwrap();

    let hive = Hive::new(&data).unwrap();
    let root = hive.root_key_node().unwrap();

    let value = root.value("BinData");
    assert!(value.is_some(), "BinData should exist");

    let val = value.unwrap().unwrap();
    assert_eq!(val.data_type().unwrap(), KeyValueDataType::RegBinary);
}

#[test]
fn test_complex_hierarchy() {
    let mut builder = HiveBuilder::new("SYSTEM").unwrap();
    let root = builder.root_key();

    // Создаем иерархию как в реальном SYSTEM hive
    let control_set = builder.create_subkey(&root, "ControlSet001").unwrap();
    let control = builder.create_subkey(&control_set, "Control").unwrap();
    let sgo = builder
        .create_subkey(&control, "ServiceGroupOrder")
        .unwrap();

    // Добавляем значения
    builder
        .set_dword(&control_set, "CurrentControlSet", 1)
        .unwrap();
    builder
        .set_string(&sgo, "Description", "Service Group Order")
        .unwrap();

    let data = builder.build().unwrap();

    // Проверяем что все читается
    let hive = Hive::new(&data).unwrap();
    let root = hive.root_key_node().unwrap();

    // Навигация по пути
    let cs = root.subkey("ControlSet001").unwrap().unwrap();
    let ctrl = cs.subkey("Control").unwrap().unwrap();
    let sgo = ctrl.subkey("ServiceGroupOrder").unwrap().unwrap();

    // Проверяем значение
    let desc = sgo.value("Description");
    assert!(desc.is_some(), "Description should exist");
}

#[test]
fn test_from_bytes() {
    // Создаем hive
    let mut builder = HiveBuilder::new("ORIGINAL").unwrap();
    let root = builder.root_key();
    builder.set_dword(&root, "Value1", 100).unwrap();

    let data = builder.build().unwrap();

    // Загружаем обратно
    let mut builder2 = HiveBuilder::from_bytes(&data).unwrap();
    let root2 = builder2.root_key();

    // Добавляем новое значение
    builder2.set_dword(&root2, "Value2", 200).unwrap();

    let data2 = builder2.build().unwrap();

    // Проверяем что оба значения есть
    let hive = Hive::new(&data2).unwrap();
    let root = hive.root_key_node().unwrap();

    let v1 = root.value("Value1").unwrap().unwrap();
    let v2 = root.value("Value2").unwrap().unwrap();

    assert_eq!(v1.dword_data().unwrap(), 100);
    assert_eq!(v2.dword_data().unwrap(), 200);
}

#[test]
fn test_hive_expansion() {
    let mut builder = HiveBuilder::new("EXPAND").unwrap();
    let root = builder.root_key();

    // Создаем много подключей чтобы заставить hive расшириться
    for i in 0..50 {
        let name = format!("Key{:03}", i);
        let key = builder.create_subkey(&root, &name).unwrap();
        builder.set_dword(&key, "Index", i as u32).unwrap();
    }

    let data = builder.build().unwrap();

    // Размер должен быть больше одного bin
    assert!(data.len() > 8192, "Hive should have expanded");

    // Проверяем что все ключи читаются
    let hive = Hive::new(&data).unwrap();
    let root = hive.root_key_node().unwrap();

    // Проверяем случайные ключи
    let key10 = root.subkey("Key010");
    assert!(key10.is_some(), "Key010 should exist");

    let key49 = root.subkey("Key049");
    assert!(key49.is_some(), "Key049 should exist");
}

#[test]
fn test_multiple_values_on_key() {
    let mut builder = HiveBuilder::new("TEST").unwrap();
    let root = builder.root_key();

    // Добавляем несколько значений к одному ключу
    builder.set_dword(&root, "Value1", 1).unwrap();
    builder.set_dword(&root, "Value2", 2).unwrap();
    builder.set_dword(&root, "Value3", 3).unwrap();
    builder.set_string(&root, "Name", "Test Key").unwrap();

    let data = builder.build().unwrap();

    let hive = Hive::new(&data).unwrap();
    let root = hive.root_key_node().unwrap();

    // Проверяем все значения
    assert_eq!(
        root.value("Value1").unwrap().unwrap().dword_data().unwrap(),
        1
    );
    assert_eq!(
        root.value("Value2").unwrap().unwrap().dword_data().unwrap(),
        2
    );
    assert_eq!(
        root.value("Value3").unwrap().unwrap().dword_data().unwrap(),
        3
    );
    assert!(root.value("Name").is_some());
}

#[test]
fn test_small_inline_data() {
    let mut builder = HiveBuilder::new("TEST").unwrap();
    let root = builder.root_key();

    // Маленькие данные (<= 4 байт) хранятся inline в data_offset
    builder.set_binary(&root, "Tiny", &[0x12, 0x34]).unwrap();

    let data = builder.build().unwrap();

    let hive = Hive::new(&data).unwrap();
    let root = hive.root_key_node().unwrap();

    let val = root.value("Tiny").unwrap().unwrap();
    let raw_data = val.data().unwrap();

    match raw_data {
        hive::KeyValueData::Small(bytes) => {
            assert_eq!(bytes.len(), 2);
            assert_eq!(bytes[0], 0x12);
            assert_eq!(bytes[1], 0x34);
        },
        _ => panic!("Expected small inline data"),
    }
}
