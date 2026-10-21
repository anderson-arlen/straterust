use super::*;
impl<R: Read + Seek> Archive<R> {
    /// Old installer archives have no filename directory. Discover bounded,
    /// encrypted and unencrypted nested archives by their decoded signature instead.
    pub fn embedded_archives(&mut self, max_bytes: usize) -> Result<Vec<(usize, Vec<u8>)>> {
        let mut found = Vec::new();
        let mut total = 0usize;
        for index in 0..self.blocks.len() {
            let block = self.blocks[index];
            if block.flags & EXISTS == 0 || block.size < 32 || block.size as usize > max_bytes {
                continue;
            }
            let key = if block.flags & ENCRYPTED != 0 {
                if block.flags & (IMPLODE | COMPRESS) == 0 {
                    continue;
                }
                let count = (block.size as usize).div_ceil(self.metadata.sector_size as usize);
                let length = (count + 1) * 4;
                if length > block.packed_size as usize {
                    continue;
                }
                let encrypted = read_at(
                    &mut self.reader,
                    self.archive_start + u64::from(block.offset),
                    length,
                )?;
                let combined = (u32_at(&encrypted, 0) ^ length as u32).wrapping_sub(0xeeee_eeee);
                let key = (0..256).find_map(|i| {
                    let key = combined.wrapping_sub(CRYPT[1024 + i]);
                    let mut table = encrypted.clone();
                    decrypt(&mut table, key);
                    let offsets: Vec<_> = table
                        .as_chunks::<4>()
                        .0
                        .iter()
                        .map(|b| u32_at(b, 0))
                        .collect();
                    (offsets[0] == length as u32
                        && offsets[count] == block.packed_size
                        && offsets
                            .windows(2)
                            .all(|p| p[0] < p[1] && p[1] - p[0] <= self.metadata.sector_size))
                    .then_some(key.wrapping_add(1))
                });
                let Some(key) = key else {
                    continue;
                };
                Some(key)
            } else {
                None
            };
            let bytes = self.read_block(block, "unnamed", max_bytes, key)?;
            if bytes.starts_with(b"MPQ\x1a") {
                total += bytes.len();
                ensure!(
                    found.len() < 1024 && total <= 256 * 1024 * 1024,
                    "embedded archives exceed import limits"
                );
                found.push((index, bytes));
            }
        }
        Ok(found)
    }
}
