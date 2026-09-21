//! Storage tests: T-G01 and T-G02.

use std::io::Cursor;
use std::sync::Arc;
use tick_compare::contracts::types::*;
use tick_compare::storage::logger::*;
use tick_compare::storage::reader::*;

#[test]
fn test_tg01_roundtrip_all_record_kinds() {
    let run_id = RunId([0x42; 16]);

    let raw = LogRecord::RawFrame(LogRawFrame {
        broker_id: 1,
        connection_generation: 1,
        frame_index: 10,
        rx_mono_ns: MonoNs(123456789),
        rx_unix_ns: Some(1700000000000),
        config_epoch: 1,
        analysis_segment: 1,
        raw_wire_bytes: Arc::new(vec![0xAA, 0xBB, 0xCC, 0xDD]),
        dispositions: vec![SequenceDisposition::New, SequenceDisposition::DuplicateExact],
    });

    let meta = LogRecord::Metadata(LogMetadata {
        config_epoch: 1,
        observed_mono_ns: MonoNs(100),
        toml_text: "[test]\nkey = 'value'\n".to_string(),
    });

    let diag = LogRecord::Diagnostic(Diagnostic {
        code: "SEQUENCE_GAP".to_string(),
        severity: DiagnosticSeverity::Warn,
        broker_id: 1,
        session_id: Some(10),
        mono_ns: MonoNs(200),
        sequence_range: Some((100, 105)),
        known_count: Some(5),
        detail_value: -1,
        message: "Missing sequences 100..105".to_string(),
    });

    let mut stream_bytes = Vec::new();
    let file_header = create_file_header(run_id, 1, 1);
    stream_bytes.extend_from_slice(&file_header);

    let bytes_raw = encode_record(&raw, 0).unwrap();
    let bytes_meta = encode_record(&meta, 1).unwrap();
    let bytes_diag = encode_record(&diag, 2).unwrap();

    stream_bytes.extend_from_slice(&bytes_raw);
    stream_bytes.extend_from_slice(&bytes_meta);
    stream_bytes.extend_from_slice(&bytes_diag);

    let cursor = Cursor::new(stream_bytes);
    let mut reader = LogFileReader::new(cursor).expect("Must parse header");

    assert_eq!(reader.run_id(), run_id);
    assert_eq!(reader.file_id(), 1);
    assert_eq!(reader.broker_id(), 1);

    // Read 1: RawFrame
    match reader.next_record() {
        ReadResult::Record(LogRecord::RawFrame(r)) => {
            assert_eq!(r.broker_id, 1);
            assert_eq!(r.frame_index, 10);
            assert_eq!(r.rx_mono_ns, MonoNs(123456789));
            assert_eq!(r.rx_unix_ns, Some(1700000000000));
            assert_eq!(*r.raw_wire_bytes, vec![0xAA, 0xBB, 0xCC, 0xDD]);
            assert_eq!(r.dispositions.len(), 2);
        }
        other => panic!("Expected RawFrame, got {:?}", other),
    }

    // Read 2: Metadata
    match reader.next_record() {
        ReadResult::Record(LogRecord::Metadata(m)) => {
            assert_eq!(m.config_epoch, 1);
            assert_eq!(m.toml_text, "[test]\nkey = 'value'\n");
        }
        other => panic!("Expected Metadata, got {:?}", other),
    }

    // Read 3: Diagnostic
    match reader.next_record() {
        ReadResult::Record(LogRecord::Diagnostic(d)) => {
            assert_eq!(d.code, "SEQUENCE_GAP");
            assert_eq!(d.severity, DiagnosticSeverity::Warn);
            assert_eq!(d.sequence_range, Some((100, 105)));
        }
        other => panic!("Expected Diagnostic, got {:?}", other),
    }

    // Read 4: Clean EOF
    assert_eq!(reader.next_record(), ReadResult::CleanEof);
}

#[test]
fn test_tg02_corrupt_and_truncated_tail() {
    let run_id = RunId([1u8; 16]);
    let meta = LogRecord::Metadata(LogMetadata {
        config_epoch: 1,
        observed_mono_ns: MonoNs(50),
        toml_text: "test".to_string(),
    });

    let mut stream_bytes = Vec::new();
    stream_bytes.extend_from_slice(&create_file_header(run_id, 1, 0));
    let mut record_bytes = encode_record(&meta, 0).unwrap();

    // Corrupt one byte of payload
    record_bytes[RECORD_ENVELOPE_LEN] ^= 0xFF;
    stream_bytes.extend_from_slice(&record_bytes);

    let cursor = Cursor::new(stream_bytes);
    let mut reader = LogFileReader::new(cursor).unwrap();
    let res = reader.next_record();
    assert!(matches!(res, ReadResult::Corrupt(msg) if msg.contains("CRC32C mismatch")));

    // Test truncated tail
    let mut partial_bytes = Vec::new();
    partial_bytes.extend_from_slice(&create_file_header(run_id, 2, 0));
    let valid_record = encode_record(&meta, 0).unwrap();
    partial_bytes.extend_from_slice(&valid_record[..RECORD_ENVELOPE_LEN + 2]); // Cut off payload!

    let cursor2 = Cursor::new(partial_bytes);
    let mut reader2 = LogFileReader::new(cursor2).unwrap();
    let res2 = reader2.next_record();
    assert!(matches!(res2, ReadResult::TruncatedTail(_)));
}
