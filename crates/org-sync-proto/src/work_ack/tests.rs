use super::*;

fn ack() -> WorkAck {
    WorkAck {
        origin_office_id: OfficeId::new("OFF1").unwrap(),
        incarnation: OriginIncarnation::new("INC1").unwrap(),
        from_exclusive: 0,
        through_seq: 3,
        rejected: vec![WorkRejection {
            seq: 2,
            code: "not_owner".into(),
        }],
    }
}
#[test]
fn ack_rejection_ranges_codes_and_credit_are_validated() {
    assert!(ack().validate().is_ok());
    for case in 0..9 {
        let mut a = ack();
        match case {
            0 => a.from_exclusive = 3,
            1 => a.rejected[0].seq = 0,
            2 => a.rejected[0].seq = 4,
            3 => a.rejected.push(a.rejected[0].clone()),
            4 => {
                a.rejected = vec![
                    WorkRejection {
                        seq: 3,
                        code: "x".into(),
                    },
                    WorkRejection {
                        seq: 2,
                        code: "x".into(),
                    },
                ]
            }
            5 => a.rejected[0].code.clear(),
            6 => a.rejected[0].code = "a".repeat(65),
            7 => a.rejected[0].code = "Not_Owner".into(),
            _ => {
                a.through_seq = 33;
                a.rejected = (1..=33)
                    .map(|seq| WorkRejection {
                        seq,
                        code: "x".into(),
                    })
                    .collect();
            }
        }
        assert_eq!(a.validate(), Err(InvalidValue));
        assert_eq!(work_ack_proof_digest_v1(&a), Err(InvalidValue));
    }
}

#[test]
fn independent_python_work_ack_known_answers_bind_base_and_rejected_set() {
    let rows: serde_json::Value =
        serde_json::from_str(include_str!("../../test-vectors/work-ack-v1.json")).unwrap();
    let rows = rows.as_array().unwrap();
    assert_eq!(rows.len(), 2);
    for row in rows {
        let ack = WorkAck {
            origin_office_id: OfficeId::new(row["office"].as_str().unwrap()).unwrap(),
            incarnation: OriginIncarnation::new(row["incarnation"].as_str().unwrap()).unwrap(),
            from_exclusive: row["from_exclusive"].as_u64().unwrap(),
            through_seq: row["through_seq"].as_u64().unwrap(),
            rejected: row["rejected"]
                .as_array()
                .unwrap()
                .iter()
                .map(|r| WorkRejection {
                    seq: r["seq"].as_u64().unwrap(),
                    code: r["code"].as_str().unwrap().into(),
                })
                .collect(),
        };
        let digest = work_ack_proof_digest_v1(&ack).unwrap();
        let actual = digest
            .as_bytes()
            .iter()
            .map(|b| format!("{b:02x}"))
            .collect::<String>();
        assert_eq!(actual, row["digest_hex"].as_str().unwrap());
    }
}

#[test]
fn maximum_credit_and_code_bounds_are_inclusive_and_unknown_codes_are_preserved() {
    let mut a = ack();
    a.through_seq = 32;
    a.rejected = (1..=32)
        .map(|seq| WorkRejection {
            seq,
            code: "a".repeat(64),
        })
        .collect();
    assert!(a.validate().is_ok());
    for code in ["x", "unknown_future_42", "0", "_"] {
        assert!(work_rejection_code_valid(code));
    }
    for code in ["", "bad-code", "bad.code", "É", "UPPER", "a b", "a\n"] {
        assert!(!work_rejection_code_valid(code));
    }
}
