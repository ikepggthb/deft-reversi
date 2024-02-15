
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
