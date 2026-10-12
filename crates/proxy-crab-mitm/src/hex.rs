//! 摘要的小写十六进制编码。
//!
//! sha2 0.11 的摘要类型（hybrid-array）不再实现 `LowerHex`，无法直接 `format!("{:x}")`。

/// 把字节序列编码为小写十六进制字符串。
pub(crate) fn hex_lower(bytes: &[u8]) -> String {
    const DIGITS: &[u8; 16] = b"0123456789abcdef";
    let mut text = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        text.push(DIGITS[(byte >> 4) as usize] as char);
        text.push(DIGITS[(byte & 0x0f) as usize] as char);
    }
    text
}

#[cfg(test)]
mod tests {
    use sha2::{Digest, Sha256};

    use super::*;

    /// 固定向量：asset 元数据 / 工作区迁移都把 sha256 以该格式落到磁盘，格式不能变。
    #[test]
    fn encodes_sha256_digest_as_lowercase_hex() {
        assert_eq!(
            hex_lower(&Sha256::digest(b"hello")),
            "2cf24dba5fb0a30e26e83b2ac5b9e29e1b161e5c1fa7425e73043362938b9824"
        );
        assert_eq!(hex_lower(&[0x00, 0x0f, 0xa5, 0xff]), "000fa5ff");
        assert_eq!(hex_lower(&[]), "");
    }
}
