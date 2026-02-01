use bytes::{BufMut, BytesMut};
use chrono::{DateTime, Utc};

/// Binary COPY format buffer for PostgreSQL
///
/// Provides zero-copy serialization of PostgreSQL binary COPY format.
/// Reuses a single BytesMut buffer to minimize allocations.
///
/// # Example
/// ```no_run
/// use ckbadger_indexer::db::copy_format::BinaryCopyBuffer;
///
/// let mut buf = BinaryCopyBuffer::new(3); // 3 columns
/// buf.start_row();
/// buf.write_i64(12345);
/// buf.write_text("hello");
/// buf.write_bytea(&[0x01, 0x02, 0x03]);
/// let data = buf.finish();
/// // Use data with COPY FROM STDIN BINARY
/// ```
pub struct BinaryCopyBuffer {
    buf: BytesMut,
    column_count: i16,
}

impl BinaryCopyBuffer {
    /// Create a new binary COPY buffer with the specified column count
    ///
    /// Writes the PostgreSQL binary COPY header immediately.
    ///
    /// # Arguments
    /// * `column_count` - Number of columns per row
    pub fn new(column_count: i16) -> Self {
        let mut buf = BytesMut::with_capacity(64 * 1024);
        // PostgreSQL binary COPY header
        buf.put_slice(b"PGCOPY\n\xff\r\n\0");
        buf.put_i32(0); // flags
        buf.put_i32(0); // header extension length
        Self { buf, column_count }
    }

    /// Start a new row
    ///
    /// Must be called before writing column values.
    pub fn start_row(&mut self) {
        self.buf.put_i16(self.column_count);
    }

    /// Write a NULL value
    pub fn write_null(&mut self) {
        self.buf.put_i32(-1);
    }

    /// Write BYTEA (byte array)
    ///
    /// # Arguments
    /// * `data` - Byte slice to write
    pub fn write_bytea(&mut self, data: &[u8]) {
        self.buf.put_i32(data.len() as i32);
        self.buf.put_slice(data);
    }

    /// Write INT2 (SMALLINT)
    ///
    /// # Arguments
    /// * `val` - 16-bit signed integer
    pub fn write_i16(&mut self, val: i16) {
        self.buf.put_i32(2);
        self.buf.put_i16(val);
    }

    /// Write INT4 (INTEGER)
    ///
    /// # Arguments
    /// * `val` - 32-bit signed integer
    pub fn write_i32(&mut self, val: i32) {
        self.buf.put_i32(4);
        self.buf.put_i32(val);
    }

    /// Write INT8 (BIGINT)
    ///
    /// # Arguments
    /// * `val` - 64-bit signed integer
    pub fn write_i64(&mut self, val: i64) {
        self.buf.put_i32(8);
        self.buf.put_i64(val);
    }

    /// Write BOOL (BOOLEAN)
    ///
    /// # Arguments
    /// * `val` - Boolean value
    pub fn write_bool(&mut self, val: bool) {
        self.buf.put_i32(1);
        self.buf.put_u8(if val { 1 } else { 0 });
    }

    /// Write TIMESTAMPTZ (timestamp with timezone)
    ///
    /// Encodes as microseconds since PostgreSQL epoch (2000-01-01 00:00:00 UTC).
    ///
    /// # Arguments
    /// * `dt` - DateTime in UTC
    pub fn write_timestamptz(&mut self, dt: DateTime<Utc>) {
        self.buf.put_i32(8);
        let epoch = DateTime::parse_from_rfc3339("2000-01-01T00:00:00Z")
            .expect("PostgreSQL epoch is a valid date")
            .with_timezone(&Utc);
        let duration = dt.signed_duration_since(epoch);
        let usecs = duration.num_microseconds().unwrap_or(0);
        self.buf.put_i64(usecs);
    }

    /// Write TEXT (UTF-8 string)
    ///
    /// # Arguments
    /// * `s` - String slice to write
    pub fn write_text(&mut self, s: &str) {
        self.buf.put_i32(s.len() as i32);
        self.buf.put_slice(s.as_bytes());
    }

    /// Write optional BYTEA (NULL if None)
    ///
    /// # Arguments
    /// * `data` - Optional byte slice
    pub fn write_bytea_opt(&mut self, data: Option<&[u8]>) {
        match data {
            Some(d) => self.write_bytea(d),
            None => self.write_null(),
        }
    }

    /// Write optional INT2 (NULL if None)
    ///
    /// # Arguments
    /// * `val` - Optional 16-bit signed integer
    pub fn write_i16_opt(&mut self, val: Option<i16>) {
        match val {
            Some(v) => self.write_i16(v),
            None => self.write_null(),
        }
    }

    /// Write optional INT4 (NULL if None)
    ///
    /// # Arguments
    /// * `val` - Optional 32-bit signed integer
    pub fn write_i32_opt(&mut self, val: Option<i32>) {
        match val {
            Some(v) => self.write_i32(v),
            None => self.write_null(),
        }
    }

    /// Write optional INT8 (NULL if None)
    ///
    /// # Arguments
    /// * `val` - Optional 64-bit signed integer
    pub fn write_i64_opt(&mut self, val: Option<i64>) {
        match val {
            Some(v) => self.write_i64(v),
            None => self.write_null(),
        }
    }

    /// Write optional TEXT (NULL if None)
    ///
    /// # Arguments
    /// * `s` - Optional string slice
    pub fn write_text_opt(&mut self, s: Option<&str>) {
        match s {
            Some(v) => self.write_text(v),
            None => self.write_null(),
        }
    }

    /// Write optional BOOL (NULL if None)
    ///
    /// # Arguments
    /// * `val` - Optional boolean value
    pub fn write_bool_opt(&mut self, val: Option<bool>) {
        match val {
            Some(v) => self.write_bool(v),
            None => self.write_null(),
        }
    }

    /// Write optional TIMESTAMPTZ (NULL if None)
    ///
    /// # Arguments
    /// * `dt` - Optional DateTime in UTC
    pub fn write_timestamptz_opt(&mut self, dt: Option<DateTime<Utc>>) {
        match dt {
            Some(v) => self.write_timestamptz(v),
            None => self.write_null(),
        }
    }

    /// Write JSONB (JSON binary format)
    ///
    /// PostgreSQL binary JSONB format requires a version byte prefix.
    /// Currently only version 1 is supported.
    ///
    /// # Arguments
    /// * `json_str` - Valid JSON string (will be prefixed with version byte)
    pub fn write_jsonb(&mut self, json_str: &str) {
        // JSONB binary format: 1-byte version + JSON text
        // Version 1 is the only supported version
        let json_bytes = json_str.as_bytes();
        let total_len = 1 + json_bytes.len(); // version byte + JSON content
        self.buf.put_i32(total_len as i32);
        self.buf.put_u8(1); // JSONB version 1
        self.buf.put_slice(json_bytes);
    }

    /// Write optional JSONB (NULL if None)
    ///
    /// # Arguments
    /// * `json_str` - Optional JSON string
    pub fn write_jsonb_opt(&mut self, json_str: Option<&str>) {
        match json_str {
            Some(v) => self.write_jsonb(v),
            None => self.write_null(),
        }
    }

    /// Write NUMERIC from a decimal string
    ///
    /// Encodes a numeric string (e.g., "12345678901234567890") into PostgreSQL
    /// binary NUMERIC format. Supports very large numbers (up to ~40 digits).
    ///
    /// PostgreSQL NUMERIC binary format:
    /// - ndigits (i16): number of base-10000 digit groups
    /// - weight (i16): weight of first digit (position relative to decimal point)
    /// - sign (i16): 0x0000=positive, 0x4000=negative, 0xC000=NaN
    /// - dscale (i16): display scale (decimal places to show)
    /// - digits (i16[]): base-10000 digit groups
    ///
    /// # Arguments
    /// * `s` - Numeric string (e.g., "12345", "-67890", "0")
    ///
    /// # Panics
    /// Panics if the string is not a valid integer (no decimal point support).
    pub fn write_numeric(&mut self, s: &str) {
        // Handle empty or zero
        if s.is_empty() || s == "0" || s == "-0" {
            // Special case: zero
            // ndigits=0, weight=0, sign=0, dscale=0
            self.buf.put_i32(8); // length: 4 i16 values = 8 bytes
            self.buf.put_i16(0); // ndigits
            self.buf.put_i16(0); // weight
            self.buf.put_i16(0); // sign (positive)
            self.buf.put_i16(0); // dscale
            return;
        }

        // Determine sign and get absolute value string
        let (is_negative, abs_str) = if let Some(stripped) = s.strip_prefix('-') {
            (true, stripped)
        } else {
            (false, s)
        };

        // Remove leading zeros (but keep at least one digit)
        let abs_str = abs_str.trim_start_matches('0');
        let abs_str = if abs_str.is_empty() { "0" } else { abs_str };

        // Handle zero case after stripping
        if abs_str == "0" {
            self.buf.put_i32(8);
            self.buf.put_i16(0);
            self.buf.put_i16(0);
            self.buf.put_i16(0);
            self.buf.put_i16(0);
            return;
        }

        // Convert to base-10000 digits
        // PostgreSQL NUMERIC uses base 10000 for each digit group
        let mut digits: Vec<i16> = Vec::new();
        let len = abs_str.len();

        // Process from right to left, 4 digits at a time
        let mut pos = len;
        while pos > 0 {
            let start = pos.saturating_sub(4);
            let chunk = &abs_str[start..pos];
            let val: i16 = chunk.parse().expect("Invalid numeric digit");
            digits.push(val);
            pos = start;
        }

        // Reverse to get most significant first
        digits.reverse();

        // Remove leading zero groups (shouldn't happen after trim, but be safe)
        while digits.len() > 1 && digits[0] == 0 {
            digits.remove(0);
        }

        let ndigits = digits.len() as i16;
        // Weight is the position of the first digit group relative to the decimal point
        // For integer, weight = ndigits - 1 (counting from 0)
        let weight = ndigits - 1;
        let sign: i16 = if is_negative { 0x4000 } else { 0x0000 };
        let dscale: i16 = 0; // No decimal places for integers

        // Calculate total length: header (8 bytes) + digits (2 bytes each)
        let total_len = 8 + (ndigits as i32 * 2);
        self.buf.put_i32(total_len);
        self.buf.put_i16(ndigits);
        self.buf.put_i16(weight);
        self.buf.put_i16(sign);
        self.buf.put_i16(dscale);

        for d in digits {
            self.buf.put_i16(d);
        }
    }

    /// Finalize and return the buffer
    ///
    /// Writes the PostgreSQL binary COPY trailer and consumes self.
    /// The returned buffer is ready for use with COPY FROM STDIN BINARY.
    pub fn finish(mut self) -> BytesMut {
        // PostgreSQL binary COPY trailer
        self.buf.put_i16(-1);
        self.buf
    }

    /// Take the current buffer for streaming
    ///
    /// Splits the buffer and returns the accumulated data.
    /// Useful for streaming large datasets without holding everything in memory.
    pub fn take_buffer(&mut self) -> BytesMut {
        self.buf.split()
    }

    /// Clear the buffer for reuse
    ///
    /// Resets the buffer and writes a new header.
    /// Allows reusing the same BinaryCopyBuffer instance.
    pub fn clear(&mut self) {
        self.buf.clear();
        // Rewrite header
        self.buf.put_slice(b"PGCOPY\n\xff\r\n\0");
        self.buf.put_i32(0); // flags
        self.buf.put_i32(0); // header extension length
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_binary_header() {
        let buf = BinaryCopyBuffer::new(2);
        let data = buf.finish();
        // Verify PostgreSQL binary COPY signature
        assert_eq!(&data[0..11], b"PGCOPY\n\xff\r\n\0");
        // Verify flags (4 bytes, value 0)
        assert_eq!(&data[11..15], &[0, 0, 0, 0]);
        // Verify header extension length (4 bytes, value 0)
        assert_eq!(&data[15..19], &[0, 0, 0, 0]);
    }

    #[test]
    fn test_binary_trailer() {
        let buf = BinaryCopyBuffer::new(1);
        let data = buf.finish();
        // Trailer is -1 as i16 (0xFFFF in big-endian)
        let len = data.len();
        assert_eq!(&data[len - 2..], &[0xFF, 0xFF]);
    }

    #[test]
    fn test_write_i64() {
        let mut buf = BinaryCopyBuffer::new(1);
        buf.start_row();
        buf.write_i64(12345678901234i64);
        let data = buf.finish();

        // Header (19 bytes) + column count (2 bytes) + length prefix (4 bytes) + value (8 bytes) + trailer (2 bytes)
        assert_eq!(data.len(), 19 + 2 + 4 + 8 + 2);

        // Verify column count (1 as i16 big-endian)
        assert_eq!(&data[19..21], &[0, 1]);

        // Verify length prefix (8 as i32 big-endian)
        assert_eq!(&data[21..25], &[0, 0, 0, 8]);

        // Verify value (12345678901234 as i64 big-endian)
        let expected_value: i64 = 12345678901234;
        let mut value_bytes = [0u8; 8];
        value_bytes.copy_from_slice(&data[25..33]);
        let actual_value = i64::from_be_bytes(value_bytes);
        assert_eq!(actual_value, expected_value);
    }

    #[test]
    fn test_write_i32() {
        let mut buf = BinaryCopyBuffer::new(1);
        buf.start_row();
        buf.write_i32(42);
        let data = buf.finish();

        // Header (19) + column count (2) + length (4) + value (4) + trailer (2)
        assert_eq!(data.len(), 31);

        // Verify length prefix (4 as i32 big-endian)
        assert_eq!(&data[21..25], &[0, 0, 0, 4]);

        // Verify value (42 as i32 big-endian)
        assert_eq!(&data[25..29], &[0, 0, 0, 42]);
    }

    #[test]
    fn test_write_i16() {
        let mut buf = BinaryCopyBuffer::new(1);
        buf.start_row();
        buf.write_i16(255);
        let data = buf.finish();

        // Header (19) + column count (2) + length (4) + value (2) + trailer (2)
        assert_eq!(data.len(), 29);

        // Verify length prefix (2 as i32 big-endian)
        assert_eq!(&data[21..25], &[0, 0, 0, 2]);

        // Verify value (255 as i16 big-endian)
        assert_eq!(&data[25..27], &[0, 255]);
    }

    #[test]
    fn test_write_bool() {
        let mut buf = BinaryCopyBuffer::new(2);
        buf.start_row();
        buf.write_bool(true);
        buf.write_bool(false);
        let data = buf.finish();

        // Header (19) + column count (2) + (length (4) + value (1)) * 2 + trailer (2)
        assert_eq!(data.len(), 33);

        // First bool: length = 1, value = 1
        assert_eq!(&data[21..25], &[0, 0, 0, 1]);
        assert_eq!(data[25], 1);

        // Second bool: length = 1, value = 0
        assert_eq!(&data[26..30], &[0, 0, 0, 1]);
        assert_eq!(data[30], 0);
    }

    #[test]
    fn test_write_bytea() {
        let mut buf = BinaryCopyBuffer::new(1);
        buf.start_row();
        buf.write_bytea(&[0x01, 0x02, 0x03]);
        let data = buf.finish();

        // Header (19) + column count (2) + length (4) + value (3) + trailer (2)
        assert_eq!(data.len(), 30);

        // Verify length prefix (3 as i32 big-endian)
        assert_eq!(&data[21..25], &[0, 0, 0, 3]);

        // Verify value
        assert_eq!(&data[25..28], &[0x01, 0x02, 0x03]);
    }

    #[test]
    fn test_write_text() {
        let mut buf = BinaryCopyBuffer::new(1);
        buf.start_row();
        buf.write_text("hello");
        let data = buf.finish();

        // Header (19) + column count (2) + length (4) + value (5) + trailer (2)
        assert_eq!(data.len(), 32);

        // Verify length prefix (5 as i32 big-endian)
        assert_eq!(&data[21..25], &[0, 0, 0, 5]);

        // Verify value
        assert_eq!(&data[25..30], b"hello");
    }

    #[test]
    fn test_write_null() {
        let mut buf = BinaryCopyBuffer::new(1);
        buf.start_row();
        buf.write_null();
        let data = buf.finish();

        // Header (19) + column count (2) + null marker (4) + trailer (2)
        assert_eq!(data.len(), 27);

        // Verify NULL marker (-1 as i32 big-endian = 0xFFFFFFFF)
        assert_eq!(&data[21..25], &[0xFF, 0xFF, 0xFF, 0xFF]);
    }

    #[test]
    fn test_write_timestamptz() {
        use chrono::TimeZone;
        let mut buf = BinaryCopyBuffer::new(1);
        buf.start_row();
        let dt = Utc.with_ymd_and_hms(2024, 6, 15, 12, 0, 0).unwrap();
        buf.write_timestamptz(dt);
        let data = buf.finish();

        // Header (19) + column count (2) + length (4) + value (8) + trailer (2)
        assert_eq!(data.len(), 35);

        // Verify length prefix (8 as i32 big-endian)
        assert_eq!(&data[21..25], &[0, 0, 0, 8]);

        // Verify value is a valid i64 (microseconds since 2000-01-01)
        let mut value_bytes = [0u8; 8];
        value_bytes.copy_from_slice(&data[25..33]);
        let usecs = i64::from_be_bytes(value_bytes);

        // Should be positive (after 2000-01-01)
        assert!(usecs > 0);

        // Rough sanity check: 2024-06-15 is ~24.5 years after 2000-01-01
        // ~24.5 * 365.25 * 24 * 3600 * 1_000_000 microseconds
        let expected_approx = 24.5 * 365.25 * 24.0 * 3600.0 * 1_000_000.0;
        assert!((usecs as f64 - expected_approx).abs() < expected_approx * 0.1);
    }

    #[test]
    fn test_multiple_rows() {
        let mut buf = BinaryCopyBuffer::new(2);

        // Row 1
        buf.start_row();
        buf.write_i64(100);
        buf.write_text("first");

        // Row 2
        buf.start_row();
        buf.write_i64(200);
        buf.write_text("second");

        let data = buf.finish();

        // Header (19) + (column_count (2) + row1_data) + (column_count (2) + row2_data) + trailer (2)
        // row1_data: length(4) + i64(8) + length(4) + text(5) = 21
        // row2_data: length(4) + i64(8) + length(4) + text(6) = 22
        assert_eq!(data.len(), 19 + 2 + 21 + 2 + 22 + 2);
    }

    #[test]
    fn test_optional_values() {
        let mut buf = BinaryCopyBuffer::new(4);
        buf.start_row();
        buf.write_i64_opt(Some(42));
        buf.write_i64_opt(None);
        buf.write_text_opt(Some("hello"));
        buf.write_text_opt(None);
        let data = buf.finish();

        // Header (19) + column count (2) +
        // i64 Some: length(4) + value(8) = 12
        // i64 None: null(-1) = 4
        // text Some: length(4) + value(5) = 9
        // text None: null(-1) = 4
        // trailer (2)
        assert_eq!(data.len(), 19 + 2 + 12 + 4 + 9 + 4 + 2);
    }

    #[test]
    fn test_clear_and_reuse() {
        let mut buf = BinaryCopyBuffer::new(1);

        // First use
        buf.start_row();
        buf.write_i64(100);
        let data1 = buf.take_buffer();
        assert!(!data1.is_empty());

        // Clear and reuse
        buf.clear();
        buf.start_row();
        buf.write_i64(200);
        let data2 = buf.finish();

        // Both should have valid headers
        assert_eq!(&data1[0..11], b"PGCOPY\n\xff\r\n\0");
        assert_eq!(&data2[0..11], b"PGCOPY\n\xff\r\n\0");
    }

    #[test]
    fn test_write_bool_opt() {
        let mut buf = BinaryCopyBuffer::new(2);
        buf.start_row();
        buf.write_bool_opt(Some(true));
        buf.write_bool_opt(None);
        let data = buf.finish();

        // Header (19) + column count (2) + bool Some (5) + bool None (4) + trailer (2)
        assert_eq!(data.len(), 32);
    }

    #[test]
    fn test_write_timestamptz_opt() {
        use chrono::TimeZone;
        let mut buf = BinaryCopyBuffer::new(2);
        buf.start_row();
        let dt = Utc.with_ymd_and_hms(2024, 1, 1, 0, 0, 0).unwrap();
        buf.write_timestamptz_opt(Some(dt));
        buf.write_timestamptz_opt(None);
        let data = buf.finish();

        // Header (19) + column count (2) + timestamp Some (12) + timestamp None (4) + trailer (2)
        assert_eq!(data.len(), 39);
    }

    #[test]
    fn test_write_numeric_zero() {
        let mut buf = BinaryCopyBuffer::new(1);
        buf.start_row();
        buf.write_numeric("0");
        let data = buf.finish();

        // Header (19) + column count (2) + length (4) + numeric header (8) + trailer (2) = 35
        assert_eq!(data.len(), 35);

        // Verify length prefix (8 bytes for zero numeric)
        assert_eq!(&data[21..25], &[0, 0, 0, 8]);

        // Verify numeric header: ndigits=0, weight=0, sign=0, dscale=0
        assert_eq!(&data[25..27], &[0, 0]); // ndigits
        assert_eq!(&data[27..29], &[0, 0]); // weight
        assert_eq!(&data[29..31], &[0, 0]); // sign (positive)
        assert_eq!(&data[31..33], &[0, 0]); // dscale
    }

    #[test]
    fn test_write_numeric_small() {
        let mut buf = BinaryCopyBuffer::new(1);
        buf.start_row();
        buf.write_numeric("1234");
        let data = buf.finish();

        // Header (19) + column count (2) + length (4) + numeric (8 + 2) + trailer (2) = 37
        assert_eq!(data.len(), 37);

        // Verify ndigits=1
        assert_eq!(&data[25..27], &[0, 1]);
        // Verify weight=0 (single digit group)
        assert_eq!(&data[27..29], &[0, 0]);
        // Verify sign=positive
        assert_eq!(&data[29..31], &[0, 0]);
        // Verify dscale=0
        assert_eq!(&data[31..33], &[0, 0]);
        // Verify digit: 1234 = 0x04D2
        assert_eq!(&data[33..35], &[0x04, 0xD2]);
    }

    #[test]
    fn test_write_numeric_large() {
        let mut buf = BinaryCopyBuffer::new(1);
        buf.start_row();
        buf.write_numeric("12345678");
        let data = buf.finish();

        // ndigits=2: [1234, 5678]
        // Header (19) + column count (2) + length (4) + numeric (8 + 4) + trailer (2) = 39
        assert_eq!(data.len(), 39);

        // Verify ndigits=2
        assert_eq!(&data[25..27], &[0, 2]);
        // Verify weight=1 (first digit has weight 1)
        assert_eq!(&data[27..29], &[0, 1]);
        // Verify digits: 1234, 5678
        let d1 = i16::from_be_bytes([data[33], data[34]]);
        let d2 = i16::from_be_bytes([data[35], data[36]]);
        assert_eq!(d1, 1234);
        assert_eq!(d2, 5678);
    }

    #[test]
    fn test_write_numeric_negative() {
        let mut buf = BinaryCopyBuffer::new(1);
        buf.start_row();
        buf.write_numeric("-42");
        let data = buf.finish();

        // Verify sign=0x4000 (negative)
        assert_eq!(&data[29..31], &[0x40, 0x00]);
        // Verify digit: 42
        let d1 = i16::from_be_bytes([data[33], data[34]]);
        assert_eq!(d1, 42);
    }

    #[test]
    fn test_write_numeric_very_large() {
        let mut buf = BinaryCopyBuffer::new(1);
        buf.start_row();
        buf.write_numeric("123456789012345678901234567890");
        let data = buf.finish();

        // 30 digits = 8 groups of 4 (last group partial)
        // [12, 3456, 7890, 1234, 5678, 9012, 3456, 7890]
        let ndigits = i16::from_be_bytes([data[25], data[26]]);
        assert_eq!(ndigits, 8);

        let weight = i16::from_be_bytes([data[27], data[28]]);
        assert_eq!(weight, 7);
    }

    #[test]
    fn test_write_jsonb() {
        let mut buf = BinaryCopyBuffer::new(1);
        buf.start_row();
        buf.write_jsonb(r#"{"key":"value"}"#);
        let data = buf.finish();

        // Header (19) + column count (2) + length (4) + version (1) + json (15) + trailer (2) = 43
        assert_eq!(data.len(), 43);

        // Verify length prefix (16 = 1 version byte + 15 json bytes)
        assert_eq!(&data[21..25], &[0, 0, 0, 16]);

        // Verify JSONB version byte (1)
        assert_eq!(data[25], 1);

        // Verify JSON content
        assert_eq!(&data[26..41], br#"{"key":"value"}"#);
    }

    #[test]
    fn test_write_jsonb_opt() {
        let mut buf = BinaryCopyBuffer::new(2);
        buf.start_row();
        buf.write_jsonb_opt(Some(r#"{"a":1}"#));
        buf.write_jsonb_opt(None);
        let data = buf.finish();

        // Header (19) + column count (2) + jsonb Some (4+1+7=12) + jsonb None (4) + trailer (2)
        assert_eq!(data.len(), 39);

        // Verify first JSONB version byte
        assert_eq!(data[25], 1);
    }
}
