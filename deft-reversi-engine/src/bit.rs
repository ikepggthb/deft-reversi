
#[inline(always)]
pub fn transpose(bit: u64) -> u64 {
    let t = (bit ^ (bit >> 7)) & 0x00aa00aa00aa00aa_u64;
	let bit = bit ^ t ^ (t << 7);
	let t = (bit ^ (bit >> 14)) & 0x0000cccc0000cccc_u64;
	let bit = bit ^ t ^ (t << 14);
	let t = (bit ^ (bit >> 28)) & 0x00000000f0f0f0f0_u64;
	bit ^ t ^ (t << 28)
}


#[inline(always)]
pub fn vertical_mirror(bit: u64) -> u64 {
	let bit = ((bit >>  8) & 0x00FF00FF00FF00FFu64) | ((bit <<  8) & 0xFF00FF00FF00FF00u64);
	let bit = ((bit >> 16) & 0x0000FFFF0000FFFFu64) | ((bit << 16) & 0xFFFF0000FFFF0000u64);
	((bit >> 32) & 0x00000000FFFFFFFFu64) | ((bit << 32) & 0xFFFFFFFF00000000u64)
}


#[inline(always)]
pub fn horizontal_mirror(bit: u64) -> u64 {
    let bit = ((bit >> 1) & 0x5555555555555555u64) | ((bit << 1) & 0xAAAAAAAAAAAAAAAAu64);
    let bit = ((bit >> 2) & 0x3333333333333333u64) | ((bit << 2) & 0xCCCCCCCCCCCCCCCCu64);
				   ((bit >> 4) & 0x0F0F0F0F0F0F0F0Fu64) | ((bit << 4) & 0xF0F0F0F0F0F0F0F0u64)
}

#[test]
fn test_transpose() {
    let bit: u64 = 0b10000001_01000000_00100000_00010000_00001000_00000100_00000010_00000001; // 対角線が立っている盤面
    let expected: u64 = 0b10000000_01000000_00100000_00010000_00001000_00000100_00000010_10000001; // 転置後
    assert_eq!(transpose(bit), expected);
}

#[test]
fn test_vertical_mirror() {
    let bit: u64 = 0x00000000FFFFFFFF; // 上半分が空、下半分が埋まった盤面
    let expected: u64 = 0xFFFFFFFF00000000; // 上下反転
    assert_eq!(vertical_mirror(bit), expected);
}

#[test]
fn test_horizontal_mirror() {
    let bit: u64 = 0x8000000000000001; // 左端と右端が埋まった盤面
    let expected: u64 = 0x0100000000000080; // 左右反転
    assert_eq!(horizontal_mirror(bit), expected);
}