supported! {
    #[test]
    fn credential_status_size_and_uid_zero_are_checked_independently() {
        use super::checked_uid;
        for uid in [0, 501, u32::MAX] {
            assert_eq!(checked_uid(0, uid, 12, 12).unwrap(), uid);
            assert!(checked_uid(-1, uid, 12, 12).is_err());
            for length in [0, 11, 13] {
                assert_eq!(
                    checked_uid(0, uid, length, 12).unwrap_err().kind(),
                    std::io::ErrorKind::InvalidData
                );
            }
        }
    }
}
