fn main() {
    // Test rawcode -> int
    let hpal_bytes = [0x48u8, 0x70, 0x61, 0x6C]; // "Hpal"
    let hpal_int = u32::from_be_bytes(hpal_bytes);

    println!("Hpal -> integer:");
    println!("  0x{:08X} = {}", hpal_int, hpal_int);
    println!("  radix 8: 0{:o}", hpal_int);
    println!("  Expected: 1215324524 (0x4870616C)");
    println!("  Match: {}", hpal_int == 1215324524);
    println!();

    // Test int -> rawcode
    let bytes = hpal_int.to_be_bytes();
    let s = String::from_utf8_lossy(&bytes);
    println!("1215324524 -> rawcode:");
    println!("  '{}'", s);
    println!("  Expected: 'Hpal'");
    println!("  Match: {}", s == "Hpal");
    println!();

    // Test 'A'
    let a_bytes = [0x41u8, 0, 0, 0]; // "A\0\0\0"
    let a_int = u32::from_be_bytes(a_bytes);
    println!("A -> integer:");
    println!("  {} (0x{:08X})", a_int, a_int);
    println!("  radix 8: 0{:o}", a_int);
    println!();

    // Test int -> 'A' (need to skip leading zeros)
    let a_back = a_int.to_be_bytes();
    let start = a_back.iter().position(|&b| b != 0).unwrap_or(3);
    let s = String::from_utf8_lossy(&a_back[start..]);
    println!("{} -> rawcode:", a_int);
    println!("  '{}'", s);
    println!("  Expected: 'A'");
}

