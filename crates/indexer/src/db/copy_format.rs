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
            .unwrap()
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
}
