#[cfg(test)]
use crate::headers::OrderedRange;

#[test]
fn successful_ordered_range() {
    assert!(OrderedRange::new(10..=11).is_ok())
}

#[test]
fn unsuccessful_ordered_range() {
    assert!(OrderedRange::new(11..=10).is_err())
}

#[cfg(test)]
mod content_range {
    use crate::headers::{
        InvalidOrderedRange, OrderedRange,
        content_range::{Bound, HttpContentRange, InvalidBound, Unsatisfiable},
    };

    #[test]
    fn successful_bound() {
        let sized = Bound::new(10..=20, Some(50)).unwrap();
        assert_eq!(sized.size(), Some(50));
        assert_eq!(sized.range().start(), 10);
        assert_eq!(sized.range().end(), 20);

        let unsized_ = Bound::new(10..=20, None).unwrap();
        assert_eq!(unsized_.size(), None);
    }

    #[test]
    fn unsuccessful_bound() {
        assert_eq!(
            Bound::new(11..=10, None),
            Err(InvalidBound::InvalidRange(InvalidOrderedRange {
                start: 11,
                end: 10
            }))
        );
        assert_eq!(
            Bound::new(11..=10, Some(50)),
            Err(InvalidBound::InvalidRange(InvalidOrderedRange {
                start: 11,
                end: 10
            }))
        );

        assert_eq!(
            Bound::new(10..=50, Some(20)),
            Err(InvalidBound::InvalidSize {
                range: OrderedRange::new(10..=50).unwrap(),
                size: 20
            })
        );
    }

    #[test]
    fn successful_sized_bound_parsing() {
        assert_eq!(
            "bytes 10-20/50".parse::<HttpContentRange>().unwrap(),
            HttpContentRange::Bound(Bound::new(10..=20, Some(50)).unwrap())
        );
    }

    #[test]
    fn successful_sized_bound_to_string() {
        assert_eq!(
            "bytes 10-20/50",
            &HttpContentRange::Bound(Bound::new(10..=20, Some(50)).unwrap()).to_string()
        );
    }

    #[test]
    fn successful_unsized_bound_parsing() {
        assert_eq!(
            "bytes 10-20/*".parse::<HttpContentRange>().unwrap(),
            HttpContentRange::Bound(Bound::new(10..=20, None).unwrap())
        );
    }

    #[test]
    fn successful_unsized_bound_to_string() {
        assert_eq!(
            "bytes 10-20/*",
            &HttpContentRange::Bound(Bound::new(10..=20, None).unwrap()).to_string()
        );
    }

    #[test]
    fn unit_is_case_insensitive() {
        assert_eq!(
            "BYTES 10-20/50".parse::<HttpContentRange>().unwrap(),
            HttpContentRange::Bound(Bound::new(10..=20, Some(50)).unwrap())
        );
    }

    #[test]
    fn successful_unsatisfiable_parsing() {
        assert_eq!(
            "bytes */50".parse::<HttpContentRange>().unwrap(),
            HttpContentRange::Unsatisfiable(Unsatisfiable::new(50))
        );
    }

    #[test]
    fn successful_unsatisfiable_to_string() {
        assert_eq!(
            "bytes */50",
            &HttpContentRange::Unsatisfiable(Unsatisfiable::new(50)).to_string()
        );
    }

    mod expected_range {
        use crate::headers::{
            OrderedRange,
            content_range::{Bound, HttpContentRange, Unsatisfiable},
            range::{HttpRange, RangeSpec},
        };

        #[test]
        fn successful_range_range_range_content_bound() {
            let range = HttpRange::from(RangeSpec::Range(OrderedRange::new(10..=20).unwrap()));
            let content_range = HttpContentRange::Bound(Bound::new(10..=20, Some(50)).unwrap());

            assert!(content_range.matches_requested_range(&range));
        }

        #[test]
        fn satisfiable_range_does_not_match_unsatisfiable() {
            let range = HttpRange::from(RangeSpec::Range(OrderedRange::new(10..=50).unwrap()));
            let content_range = HttpContentRange::Unsatisfiable(Unsatisfiable::new(20));

            assert!(!content_range.matches_requested_range(&range));
        }

        #[test]
        fn range_start_beyond_size_matches_unsatisfiable() {
            let range = HttpRange::from(RangeSpec::Range(OrderedRange::new(20..=50).unwrap()));
            let content_range = HttpContentRange::Unsatisfiable(Unsatisfiable::new(20));

            assert!(content_range.matches_requested_range(&range));
        }

        #[test]
        fn successful_range_starting_point_range_content_bound() {
            let range = HttpRange::from(RangeSpec::StartingPoint(10));
            let content_range = HttpContentRange::Bound(Bound::new(10..=49, Some(50)).unwrap());

            assert!(content_range.matches_requested_range(&range));
        }

        #[test]
        fn unsuccessful_range_starting_point_range_content_bound() {
            let range = HttpRange::from(RangeSpec::StartingPoint(21));
            let content_range = HttpContentRange::Unsatisfiable(Unsatisfiable::new(20));

            assert!(content_range.matches_requested_range(&range));
        }

        #[test]
        fn successful_range_suffix_range_content_bound() {
            let range = HttpRange::from(RangeSpec::Suffix(20));
            let content_range = HttpContentRange::Bound(Bound::new(0..=19, Some(20)).unwrap());

            assert!(content_range.matches_requested_range(&range));
        }

        #[test]
        fn unsuccessful_range_suffix_zero_content_unsatisfiable() {
            let range = HttpRange::from(RangeSpec::Suffix(0));
            let content_range = HttpContentRange::Unsatisfiable(Unsatisfiable::new(20));

            assert!(content_range.matches_requested_range(&range));
        }

        #[test]
        fn suffix_exceeding_size_is_not_unsatisfiable() {
            let range = HttpRange::from(RangeSpec::Suffix(50));
            let content_range = HttpContentRange::Unsatisfiable(Unsatisfiable::new(20));

            assert!(!content_range.matches_requested_range(&range));
        }

        #[test]
        fn range_with_clamped_end_matches() {
            let range = HttpRange::from(RangeSpec::Range(OrderedRange::new(0..=999).unwrap()));
            let content_range = HttpContentRange::Bound(Bound::new(0..=49, Some(50)).unwrap());

            assert!(content_range.matches_requested_range(&range));
        }

        #[test]
        fn suffix_clamped_to_file_size_matches() {
            let range = HttpRange::from(RangeSpec::Suffix(999));
            let content_range = HttpContentRange::Bound(Bound::new(0..=49, Some(50)).unwrap());

            assert!(content_range.matches_requested_range(&range));
        }

        #[test]
        fn suffix_not_at_boundary_does_not_match() {
            let range = HttpRange::from(RangeSpec::Suffix(5));
            let content_range = HttpContentRange::Bound(Bound::new(0..=4, Some(20)).unwrap());

            assert!(!content_range.matches_requested_range(&range));
        }

        #[test]
        fn suffix_length_mismatch_does_not_match() {
            let range = HttpRange::from(RangeSpec::Suffix(3));
            let content_range = HttpContentRange::Bound(Bound::new(10..=19, Some(20)).unwrap());

            assert!(!content_range.matches_requested_range(&range));
        }

        #[test]
        fn range_start_mismatch_does_not_match() {
            let range = HttpRange::from(RangeSpec::Range(OrderedRange::new(5..=20).unwrap()));
            let content_range = HttpContentRange::Bound(Bound::new(10..=20, Some(50)).unwrap());

            assert!(!content_range.matches_requested_range(&range));
        }

        #[test]
        fn suffix_with_nonzero_start_matches() {
            let range = HttpRange::from(RangeSpec::Suffix(5));
            let content_range = HttpContentRange::Bound(Bound::new(15..=19, Some(20)).unwrap());

            assert!(content_range.matches_requested_range(&range));
        }

        #[test]
        fn bound_matches_any_of_multiple_ranges() {
            let range: HttpRange = "bytes=0-4,10-14".parse().unwrap();

            let first = HttpContentRange::Bound(Bound::new(0..=4, Some(20)).unwrap());
            assert!(first.matches_requested_range(&range));
            let second = HttpContentRange::Bound(Bound::new(10..=14, Some(20)).unwrap());
            assert!(second.matches_requested_range(&range));
            let neither = HttpContentRange::Bound(Bound::new(5..=9, Some(20)).unwrap());
            assert!(!neither.matches_requested_range(&range));
        }

        #[test]
        fn unsatisfiable_matches_only_if_every_range_is_unsatisfiable() {
            let content_range = HttpContentRange::Unsatisfiable(Unsatisfiable::new(20));

            let unsatisfiable: HttpRange = "bytes=20-,30-40,-0".parse().unwrap();
            assert!(content_range.matches_requested_range(&unsatisfiable));
            let mixed: HttpRange = "bytes=20-,5-6".parse().unwrap();
            assert!(!content_range.matches_requested_range(&mixed));
        }
    }
}

#[cfg(test)]
mod range {
    use crate::headers::{
        OrderedRange, ParseHttpRangeOrContentRangeError,
        range::{HttpRange, MAX_RANGE_SET_ELEMENTS, RangeSpec},
    };

    #[test]
    fn successful_starting_parsing() {
        assert_eq!(
            "bytes=50-".parse::<HttpRange>().unwrap(),
            HttpRange::from(RangeSpec::StartingPoint(50))
        );
    }

    #[test]
    fn successful_starting_to_string() {
        assert_eq!(
            "bytes=50-",
            &HttpRange::from(RangeSpec::StartingPoint(50)).to_string()
        );
    }

    #[test]
    fn successful_range_parsing() {
        assert_eq!(
            "bytes=50-100".parse::<HttpRange>().unwrap(),
            HttpRange::from(RangeSpec::Range(OrderedRange::new(50..=100).unwrap()))
        );
    }

    #[test]
    fn successful_range_to_string() {
        assert_eq!(
            "bytes=50-100",
            &HttpRange::from(RangeSpec::Range(OrderedRange::new(50..=100).unwrap())).to_string()
        );
    }

    #[test]
    fn successful_suffix_parsing() {
        assert_eq!(
            "bytes=-100".parse::<HttpRange>().unwrap(),
            HttpRange::from(RangeSpec::Suffix(100))
        );
    }

    #[test]
    fn successful_suffix_to_string() {
        assert_eq!(
            "bytes=-100",
            &HttpRange::from(RangeSpec::Suffix(100)).to_string()
        );
    }

    #[test]
    fn empty_input() {
        assert_eq!(
            "".parse::<HttpRange>().unwrap_err(),
            ParseHttpRangeOrContentRangeError::Empty
        );
    }

    #[test]
    fn whitespace_only_input() {
        assert_eq!(
            "   ".parse::<HttpRange>().unwrap_err(),
            ParseHttpRangeOrContentRangeError::Empty
        );
    }

    #[test]
    fn missing_equals() {
        assert_eq!(
            "bytes50-100".parse::<HttpRange>().unwrap_err(),
            ParseHttpRangeOrContentRangeError::Malformed
        );
    }

    #[test]
    fn wrong_unit() {
        assert_eq!(
            "items=0-10".parse::<HttpRange>().unwrap_err(),
            ParseHttpRangeOrContentRangeError::InvalidUnit
        );
    }

    #[test]
    fn unit_is_case_insensitive() {
        assert_eq!(
            "BYTES=50-".parse::<HttpRange>().unwrap(),
            HttpRange::from(RangeSpec::StartingPoint(50))
        );
        assert_eq!(
            "Bytes=50-100".parse::<HttpRange>().unwrap(),
            HttpRange::from(RangeSpec::Range(OrderedRange::new(50..=100).unwrap()))
        );
    }

    #[test]
    fn both_empty() {
        assert_eq!(
            "bytes=-".parse::<HttpRange>().unwrap_err(),
            ParseHttpRangeOrContentRangeError::Malformed
        );
    }

    #[test]
    fn unordered_range() {
        assert!("bytes=100-50".parse::<HttpRange>().is_err());
    }

    #[test]
    fn plus_prefix_rejected_in_starting_point() {
        assert!("bytes=+50-".parse::<HttpRange>().is_err());
    }

    #[test]
    fn plus_prefix_rejected_in_suffix() {
        assert!("bytes=-+100".parse::<HttpRange>().is_err());
    }

    #[test]
    fn plus_prefix_rejected_in_range() {
        assert!("bytes=+50-100".parse::<HttpRange>().is_err());
        assert!("bytes=50-+100".parse::<HttpRange>().is_err());
    }

    #[test]
    fn suffix_zero_parsing() {
        assert_eq!(
            "bytes=-0".parse::<HttpRange>().unwrap(),
            HttpRange::from(RangeSpec::Suffix(0))
        );
    }

    #[test]
    fn starting_point_zero() {
        assert_eq!(
            "bytes=0-".parse::<HttpRange>().unwrap(),
            HttpRange::from(RangeSpec::StartingPoint(0))
        );
    }

    #[test]
    fn range_single_byte() {
        assert_eq!(
            "bytes=0-0".parse::<HttpRange>().unwrap(),
            HttpRange::from(RangeSpec::Range(OrderedRange::new(0..=0).unwrap()))
        );
    }

    fn specs(s: &str) -> Vec<RangeSpec> {
        s.parse::<HttpRange>().unwrap().specs().to_vec()
    }

    fn range(start: u64, end: u64) -> RangeSpec {
        RangeSpec::Range(OrderedRange::new(start..=end).unwrap())
    }

    #[test]
    fn multiple_ranges() {
        assert_eq!(specs("bytes=0-1,5-6"), [range(0, 1), range(5, 6)]);
        assert_eq!(specs("bytes=0-0,-1"), [range(0, 0), RangeSpec::Suffix(1)]);
        assert_eq!(
            specs("bytes=500-600,601-999"),
            [range(500, 600), range(601, 999)]
        );
    }

    #[test]
    fn multiple_ranges_keep_request_order_and_duplicates() {
        assert_eq!(
            specs("bytes=9-,-3,0-1,0-1"),
            [
                RangeSpec::StartingPoint(9),
                RangeSpec::Suffix(3),
                range(0, 1),
                range(0, 1)
            ]
        );
    }

    #[test]
    fn optional_whitespace_around_commas() {
        // The example of RFC 9110 Section 14.1.2.
        assert_eq!(
            specs("bytes= 0-999, 4500-5499, -1000"),
            [range(0, 999), range(4500, 5499), RangeSpec::Suffix(1000)]
        );
        assert_eq!(specs("bytes=0-1 ,\t5-6"), [range(0, 1), range(5, 6)]);
        assert_eq!(specs("bytes=0-1\t,  5-6  "), [range(0, 1), range(5, 6)]);
    }

    #[test]
    fn empty_list_elements_are_ignored() {
        assert_eq!(specs("bytes=0-1,,5-6"), [range(0, 1), range(5, 6)]);
        assert_eq!(specs("bytes=,0-1"), [range(0, 1)]);
        assert_eq!(specs("bytes=0-1,"), [range(0, 1)]);
        assert_eq!(specs("bytes= , ,0-1, ,"), [range(0, 1)]);
    }

    #[test]
    fn only_empty_list_elements_rejected() {
        for s in ["bytes=", "bytes=,", "bytes= , ,", "bytes=  "] {
            assert_eq!(
                s.parse::<HttpRange>().unwrap_err(),
                ParseHttpRangeOrContentRangeError::MalformedRange,
                "{s:?}"
            );
        }
    }

    #[test]
    fn whitespace_inside_range_spec_rejected() {
        for s in ["bytes=0 -1", "bytes=0- 1", "bytes=- 1", "bytes=0 1-2"] {
            assert!(s.parse::<HttpRange>().is_err(), "{s:?}");
        }
    }

    #[test]
    fn whitespace_around_unit_rejected() {
        assert_eq!(
            "bytes =0-1".parse::<HttpRange>().unwrap_err(),
            ParseHttpRangeOrContentRangeError::InvalidUnit
        );
    }

    #[test]
    fn surrounding_whitespace_is_trimmed() {
        assert_eq!(specs(" \tbytes=0-1\t "), [range(0, 1)]);
    }

    #[test]
    fn non_ows_whitespace_rejected() {
        assert!("bytes=0-1,\n5-6".parse::<HttpRange>().is_err());
        assert!("\nbytes=0-1".parse::<HttpRange>().is_err());
    }

    #[test]
    fn unit_is_case_insensitive_with_multiple_ranges() {
        assert_eq!(specs("ByTeS=0-1,5-6"), [range(0, 1), range(5, 6)]);
    }

    #[test]
    fn any_invalid_range_spec_invalidates_the_set() {
        for s in [
            "bytes=0-1,5-4",
            "bytes=0-1,x",
            "bytes=0-1,-",
            "bytes=0-1,5",
            "bytes=0-1,1-2-3",
            "bytes=0-1,+5-6",
            "bytes=0-1,-+5",
            "bytes=0-1,0x10-20",
            "bytes=0-1,٣-٤",
        ] {
            assert!(s.parse::<HttpRange>().is_err(), "{s:?}");
        }
    }

    #[test]
    fn leading_zeros() {
        assert_eq!(
            specs("bytes=000-0010,-005,07-"),
            [
                range(0, 10),
                RangeSpec::Suffix(5),
                RangeSpec::StartingPoint(7)
            ]
        );
    }

    #[test]
    fn overflowing_positions_saturate() {
        assert_eq!(
            specs("bytes=99999999999999999999999-"),
            [RangeSpec::StartingPoint(u64::MAX)]
        );
        assert_eq!(
            specs("bytes=0-99999999999999999999999"),
            [range(0, u64::MAX)]
        );
        assert_eq!(
            specs("bytes=-99999999999999999999999"),
            [RangeSpec::Suffix(u64::MAX)]
        );
        assert_eq!(
            specs("bytes=18446744073709551615-18446744073709551616"),
            [range(u64::MAX, u64::MAX)]
        );
        assert_eq!(
            specs("bytes=99999999999999999999-099999999999999999999"),
            [range(u64::MAX, u64::MAX)]
        );
    }

    #[test]
    fn overflowing_positions_followed_by_non_digits_rejected() {
        for s in [
            "bytes=184467440737095516160x-",
            "bytes=0-184467440737095516160x",
            "bytes=-99999999999999999999999abc",
            "bytes=99999999999999999999999 -",
        ] {
            assert!(
                matches!(
                    s.parse::<HttpRange>().unwrap_err(),
                    ParseHttpRangeOrContentRangeError::InvalidRangePiece(_)
                ),
                "{s}"
            );
        }
    }

    #[test]
    fn overflowing_unordered_range_rejected() {
        for s in [
            "bytes=18446744073709551616-18446744073709551615",
            "bytes=99999999999999999999999-99999999999999999999998",
            "bytes=99999999999999999999999-5",
        ] {
            assert!(matches!(
                s.parse::<HttpRange>().unwrap_err(),
                ParseHttpRangeOrContentRangeError::UnorderedRange(_)
            ));
        }
    }

    #[test]
    fn too_many_list_elements_rejected() {
        let at_limit = format!("bytes={}", vec!["0-0"; MAX_RANGE_SET_ELEMENTS].join(","));
        assert_eq!(
            at_limit.parse::<HttpRange>().unwrap().specs().len(),
            MAX_RANGE_SET_ELEMENTS
        );

        let over_limit = format!("{at_limit},0-0");
        assert_eq!(
            over_limit.parse::<HttpRange>().unwrap_err(),
            ParseHttpRangeOrContentRangeError::TooManyRanges
        );

        let empty_over_limit = format!("bytes=0-0{}", ",".repeat(MAX_RANGE_SET_ELEMENTS));
        assert_eq!(
            empty_over_limit.parse::<HttpRange>().unwrap_err(),
            ParseHttpRangeOrContentRangeError::TooManyRanges
        );
    }

    #[test]
    fn multiple_ranges_to_string() {
        let http_range = HttpRange::new(vec![
            range(0, 1),
            RangeSpec::StartingPoint(5),
            RangeSpec::Suffix(3),
        ])
        .unwrap();
        assert_eq!(http_range.to_string(), "bytes=0-1,5-,-3");
        assert_eq!(specs(&http_range.to_string()), http_range.specs());
    }

    #[test]
    fn range_set_size_is_bounded() {
        assert_eq!(HttpRange::new(Vec::new()), None);
        assert_eq!(
            HttpRange::new(vec![RangeSpec::StartingPoint(0); MAX_RANGE_SET_ELEMENTS])
                .unwrap()
                .specs()
                .len(),
            MAX_RANGE_SET_ELEMENTS
        );
        assert_eq!(
            HttpRange::new(vec![
                RangeSpec::StartingPoint(0);
                MAX_RANGE_SET_ELEMENTS + 1
            ]),
            None
        );
    }
}

#[cfg(test)]
mod content_range_parsing_errors {
    use crate::headers::{ParseHttpRangeOrContentRangeError, content_range::HttpContentRange};

    #[test]
    fn empty_input() {
        assert_eq!(
            "".parse::<HttpContentRange>().unwrap_err(),
            ParseHttpRangeOrContentRangeError::Empty
        );
    }

    #[test]
    fn missing_space() {
        assert_eq!(
            "bytes10-20/50".parse::<HttpContentRange>().unwrap_err(),
            ParseHttpRangeOrContentRangeError::Malformed
        );
    }

    #[test]
    fn wrong_unit() {
        assert_eq!(
            "items 10-20/50".parse::<HttpContentRange>().unwrap_err(),
            ParseHttpRangeOrContentRangeError::InvalidUnit
        );
    }

    #[test]
    fn missing_slash() {
        assert_eq!(
            "bytes 10-20".parse::<HttpContentRange>().unwrap_err(),
            ParseHttpRangeOrContentRangeError::Malformed
        );
    }

    #[test]
    fn star_star() {
        assert_eq!(
            "bytes */*".parse::<HttpContentRange>().unwrap_err(),
            ParseHttpRangeOrContentRangeError::Malformed
        );
    }

    #[test]
    fn end_exceeds_size() {
        assert_eq!(
            "bytes 10-20/15".parse::<HttpContentRange>().unwrap_err(),
            ParseHttpRangeOrContentRangeError::MalformedRange
        );
    }

    #[test]
    fn end_equals_size() {
        assert_eq!(
            "bytes 0-20/20".parse::<HttpContentRange>().unwrap_err(),
            ParseHttpRangeOrContentRangeError::MalformedRange
        );
    }

    #[test]
    fn end_at_boundary() {
        assert!("bytes 0-19/20".parse::<HttpContentRange>().is_ok());
    }
}

#[cfg(test)]
mod file_range {
    use crate::{
        file_range,
        headers::{
            OrderedRange,
            content_range::{Bound, HttpContentRange, Unsatisfiable},
            range::{HttpRange, MAX_RANGE_SET_ELEMENTS, RangeSpec},
        },
    };

    #[test]
    fn no_range_returns_full_file() {
        let result = file_range(10, None).unwrap();
        assert!(result.header().is_none());
        assert_eq!(result.range(), &(0..10));
    }

    #[test]
    fn starting_point_zero() {
        let result = file_range(10, Some(HttpRange::from(RangeSpec::StartingPoint(0)))).unwrap();
        assert!(result.header().is_some());
        assert_eq!(result.range(), &(0..10));
    }

    #[test]
    fn starting_point_middle() {
        let result = file_range(10, Some(HttpRange::from(RangeSpec::StartingPoint(5)))).unwrap();
        assert_eq!(result.range(), &(5..10));
    }

    #[test]
    fn starting_point_last_byte() {
        let result = file_range(10, Some(HttpRange::from(RangeSpec::StartingPoint(9)))).unwrap();
        assert_eq!(result.range(), &(9..10));
    }

    #[test]
    fn starting_point_at_size() {
        let result = file_range(10, Some(HttpRange::from(RangeSpec::StartingPoint(10))));
        assert!(result.is_err());
    }

    #[test]
    fn starting_point_beyond_size() {
        let result = file_range(10, Some(HttpRange::from(RangeSpec::StartingPoint(20))));
        assert!(result.is_err());
    }

    #[test]
    fn range_single_byte() {
        let range = HttpRange::from(RangeSpec::Range(OrderedRange::new(0..=0).unwrap()));
        let result = file_range(10, Some(range)).unwrap();
        assert_eq!(result.range(), &(0..1));
    }

    #[test]
    fn range_full_file() {
        let range = HttpRange::from(RangeSpec::Range(OrderedRange::new(0..=9).unwrap()));
        let result = file_range(10, Some(range)).unwrap();
        assert_eq!(result.range(), &(0..10));
    }

    #[test]
    fn range_end_at_size_is_clamped() {
        let range = HttpRange::from(RangeSpec::Range(OrderedRange::new(0..=10).unwrap()));
        let result = file_range(10, Some(range)).unwrap();
        assert_eq!(result.range(), &(0..10));
    }

    #[test]
    fn range_beyond_size_is_clamped() {
        let range = HttpRange::from(RangeSpec::Range(OrderedRange::new(0..=50).unwrap()));
        let result = file_range(10, Some(range)).unwrap();
        assert_eq!(result.range(), &(0..10));
    }

    #[test]
    fn range_end_at_u64_max_is_clamped() {
        let range = HttpRange::from(RangeSpec::Range(OrderedRange::new(0..=u64::MAX).unwrap()));
        let result = file_range(10, Some(range)).unwrap();
        assert_eq!(result.range(), &(0..10));
    }

    #[test]
    fn range_start_at_size_is_unsatisfiable() {
        let range = HttpRange::from(RangeSpec::Range(OrderedRange::new(10..=20).unwrap()));
        let result = file_range(10, Some(range));
        assert!(result.is_err());
    }

    #[test]
    fn range_start_beyond_size_is_unsatisfiable() {
        let range = HttpRange::from(RangeSpec::Range(OrderedRange::new(50..=100).unwrap()));
        let result = file_range(10, Some(range));
        assert!(result.is_err());
    }

    #[test]
    fn suffix_entire_file() {
        let result = file_range(10, Some(HttpRange::from(RangeSpec::Suffix(10)))).unwrap();
        assert_eq!(result.range(), &(0..10));
    }

    #[test]
    fn suffix_last_byte() {
        let result = file_range(10, Some(HttpRange::from(RangeSpec::Suffix(1)))).unwrap();
        assert_eq!(result.range(), &(9..10));
    }

    #[test]
    fn suffix_middle() {
        let result = file_range(10, Some(HttpRange::from(RangeSpec::Suffix(5)))).unwrap();
        assert_eq!(result.range(), &(5..10));
    }

    #[test]
    fn suffix_zero_is_unsatisfiable() {
        let result = file_range(10, Some(HttpRange::from(RangeSpec::Suffix(0))));
        assert!(result.is_err());
    }

    #[test]
    fn suffix_exceeds_size_is_clamped() {
        let result = file_range(10, Some(HttpRange::from(RangeSpec::Suffix(11)))).unwrap();
        assert_eq!(result.range(), &(0..10));
    }

    #[test]
    fn size_one_no_range() {
        let result = file_range(1, None).unwrap();
        assert_eq!(result.range(), &(0..1));
    }

    #[test]
    fn size_one_starting_point_zero() {
        let result = file_range(1, Some(HttpRange::from(RangeSpec::StartingPoint(0)))).unwrap();
        assert_eq!(result.range(), &(0..1));
    }

    #[test]
    fn size_one_suffix_one() {
        let result = file_range(1, Some(HttpRange::from(RangeSpec::Suffix(1)))).unwrap();
        assert_eq!(result.range(), &(0..1));
    }

    #[test]
    fn content_range_header_present_for_range_request() {
        let result = file_range(10, Some(HttpRange::from(RangeSpec::StartingPoint(0)))).unwrap();
        let header = result.header().unwrap();
        assert_eq!(
            header,
            HttpContentRange::Bound(Bound::new(0..=9, Some(10)).unwrap())
        );
    }

    #[test]
    fn size_zero_no_range() {
        let result = file_range(0, None).unwrap();
        assert!(result.header().is_none());
        assert_eq!(result.range(), &(0..0));
    }

    #[test]
    fn size_zero_starting_point_is_unsatisfiable() {
        let result = file_range(0, Some(HttpRange::from(RangeSpec::StartingPoint(0))));
        assert!(result.is_err());
    }

    #[test]
    fn size_zero_range_is_unsatisfiable() {
        let range = HttpRange::from(RangeSpec::Range(OrderedRange::new(0..=0).unwrap()));
        let result = file_range(0, Some(range));
        assert!(result.is_err());
    }

    #[test]
    fn size_zero_suffix_serves_full_response() {
        let result = file_range(0, Some(HttpRange::from(RangeSpec::Suffix(1)))).unwrap();
        assert!(result.header().is_none());
        assert_eq!(result.range(), &(0..0));
    }

    #[test]
    fn size_zero_suffix_zero_is_unsatisfiable() {
        let result = file_range(0, Some(HttpRange::from(RangeSpec::Suffix(0))));
        assert!(result.is_err());
    }

    fn parsed(s: &str) -> Option<HttpRange> {
        Some(s.parse().unwrap())
    }

    fn single(size: u64, s: &str) -> std::ops::Range<u64> {
        let result = file_range(size, parsed(s)).unwrap();
        let range = result.range().clone();
        assert_eq!(
            result.header(),
            Some(HttpContentRange::Bound(
                Bound::new(range.start..=range.end - 1, Some(size)).unwrap()
            )),
            "{s:?}"
        );
        range
    }

    #[test]
    fn multiple_unsatisfiable_ranges_are_unsatisfiable() {
        for s in ["bytes=10-,20-30,-0", "bytes=-0,-0", "bytes=10-10,11-"] {
            let err = file_range(10, parsed(s)).unwrap_err();
            assert_eq!(
                err.header(),
                HttpContentRange::Unsatisfiable(Unsatisfiable::new(10)),
                "{s:?}"
            );
        }
    }

    #[test]
    fn unsatisfiable_ranges_are_dropped() {
        assert_eq!(single(10, "bytes=50-60,2-3"), 2..4);
        assert_eq!(single(10, "bytes=-0,2-3,10-"), 2..4);
    }

    #[test]
    fn overlapping_ranges_are_coalesced() {
        assert_eq!(single(100, "bytes=0-50,30-80"), 0..81);
        assert_eq!(single(100, "bytes=0-,0-,0-"), 0..100);
        assert_eq!(single(100, "bytes=40-60,-70,0-9"), 0..100);
    }

    #[test]
    fn unordered_overlapping_ranges_are_coalesced() {
        assert_eq!(single(100, "bytes=50-59,0-49,45-55"), 0..60);
    }

    #[test]
    fn adjacent_ranges_are_coalesced() {
        assert_eq!(single(100, "bytes=0-1,2-3"), 0..4);
        assert_eq!(single(10000, "bytes=500-600,601-999"), 500..1000);
    }

    #[test]
    fn ranges_separated_by_a_small_gap_are_coalesced() {
        // The gap is smaller than the overhead of one more part.
        assert_eq!(single(1000, "bytes=0-0,10-10"), 0..11);
        assert_eq!(single(1000, "bytes=0-0,37-37"), 0..38);
    }

    #[test]
    fn many_overlapping_ranges_are_coalesced() {
        let many = format!("bytes={}", vec!["0-"; MAX_RANGE_SET_ELEMENTS].join(","));
        assert_eq!(single(1000, &many), 0..1000);
    }

    #[test]
    fn overflowing_positions() {
        assert_eq!(single(10, "bytes=0-99999999999999999999"), 0..10);
        assert_eq!(single(10, "bytes=-99999999999999999999"), 0..10);
        assert!(file_range(10, parsed("bytes=99999999999999999999-")).is_err());
        assert!(file_range(u64::MAX, parsed("bytes=99999999999999999999-")).is_err());
    }

    #[test]
    fn disjoint_ranges_serve_full_response() {
        for s in ["bytes=0-0,-1", "bytes=0-0,38-38", "bytes=50-59,0-9"] {
            let result = file_range(100, parsed(s)).unwrap();
            assert!(result.header().is_none(), "{s:?}");
            assert_eq!(result.range(), &(0..100));
        }
    }

    #[test]
    fn size_zero_satisfiable_ranges_serve_full_response() {
        let result = file_range(0, parsed("bytes=0-1,-5")).unwrap();
        assert!(result.header().is_none());
        assert_eq!(result.range(), &(0..0));
    }

    #[test]
    fn size_zero_unsatisfiable_ranges_are_unsatisfiable() {
        let err = file_range(0, parsed("bytes=0-1,-0,0-")).unwrap_err();
        assert_eq!(
            err.header(),
            HttpContentRange::Unsatisfiable(Unsatisfiable::new(0))
        );
    }
}

#[cfg(test)]
mod serve_file {
    use bytes::Bytes;

    use crate::{
        headers::range::{HttpRange, RangeSpec},
        serve_file_with_http_range,
    };

    #[test]
    fn no_range_returns_full_body() {
        let body = Bytes::from_static(b"hello world");
        let result = serve_file_with_http_range(body.clone(), None).unwrap();
        assert_eq!(result.body(), &body);
        assert!(result.header().is_none());
    }

    #[test]
    fn range_slices_body() {
        let body = Bytes::from_static(b"hello world");
        let range = Some(HttpRange::from(RangeSpec::StartingPoint(6)));
        let result = serve_file_with_http_range(body, range).unwrap();
        assert_eq!(result.body(), &Bytes::from_static(b"world"));
        assert!(result.header().is_some());
    }

    #[test]
    fn suffix_slices_from_end() {
        let body = Bytes::from_static(b"hello world");
        let range = Some(HttpRange::from(RangeSpec::Suffix(5)));
        let result = serve_file_with_http_range(body, range).unwrap();
        assert_eq!(result.body(), &Bytes::from_static(b"world"));
    }

    #[test]
    fn empty_body_returns_empty_response() {
        let body = Bytes::new();
        let result = serve_file_with_http_range(body, None).unwrap();
        assert!(result.body().is_empty());
        assert!(result.header().is_none());
    }

    #[test]
    fn suffix_zero_is_unsatisfiable() {
        let body = Bytes::from_static(b"hello");
        let result = serve_file_with_http_range(body, Some(HttpRange::from(RangeSpec::Suffix(0))));
        assert!(result.is_err());
    }

    #[test]
    fn empty_body_with_suffix_returns_empty_response() {
        let body = Bytes::new();
        let result =
            serve_file_with_http_range(body, Some(HttpRange::from(RangeSpec::Suffix(5)))).unwrap();
        assert!(result.body().is_empty());
        assert!(result.header().is_none());
    }
}

#[cfg(feature = "axum")]
mod axum_range_extractor {
    use std::{
        pin::pin,
        task::{Context, Poll, Waker},
    };

    use axum_core::extract::OptionalFromRequestParts;
    use http::{Method, Request, header::RANGE};

    use crate::headers::{
        OrderedRange,
        range::{HttpRange, RangeSpec},
    };

    fn extract(method: Method, range: Option<&str>) -> Option<HttpRange> {
        let mut builder = Request::builder().method(method);
        if let Some(range) = range {
            builder = builder.header(RANGE, range);
        }
        let (mut parts, ()) = builder.body(()).unwrap().into_parts();

        let fut =
            pin!(<HttpRange as OptionalFromRequestParts<()>>::from_request_parts(&mut parts, &()));
        match fut.poll(&mut Context::from_waker(Waker::noop())) {
            Poll::Ready(Ok(range)) => range,
            Poll::Ready(Err(infallible)) => match infallible {},
            Poll::Pending => unreachable!("the extractor never awaits"),
        }
    }

    #[test]
    fn get_request_extracts_range() {
        assert_eq!(
            extract(Method::GET, Some("bytes=0-10")),
            Some(HttpRange::from(RangeSpec::Range(
                OrderedRange::new(0..=10).unwrap()
            )))
        );
    }

    #[test]
    fn non_get_request_ignores_range() {
        assert_eq!(extract(Method::POST, Some("bytes=0-10")), None);
        assert_eq!(extract(Method::PUT, Some("bytes=0-10")), None);
    }

    #[test]
    fn missing_range_extracts_none() {
        assert_eq!(extract(Method::GET, None), None);
    }

    #[test]
    fn malformed_range_is_ignored() {
        assert_eq!(extract(Method::GET, Some("bytes=10-0")), None);
    }

    #[test]
    fn multiple_ranges_are_extracted() {
        assert_eq!(
            extract(Method::GET, Some("bytes=0-1, 5-6,,-3")),
            HttpRange::new(vec![
                RangeSpec::Range(OrderedRange::new(0..=1).unwrap()),
                RangeSpec::Range(OrderedRange::new(5..=6).unwrap()),
                RangeSpec::Suffix(3),
            ])
        );
    }

    #[test]
    fn non_get_request_ignores_multiple_ranges() {
        assert_eq!(extract(Method::HEAD, Some("bytes=0-1,5-6")), None);
        assert_eq!(extract(Method::POST, Some("bytes=0-1,5-6")), None);
    }

    #[test]
    fn invalid_ranges_are_ignored() {
        assert_eq!(extract(Method::GET, Some("bytes=0-1,5-4")), None);
        assert_eq!(extract(Method::GET, Some("bytes=0-1,x")), None);
        assert_eq!(extract(Method::GET, Some("items=0-1,5-6")), None);
        assert_eq!(extract(Method::GET, Some("bytes=,")), None);
    }

    #[test]
    fn too_many_ranges_are_ignored() {
        let many = format!("bytes={}", vec!["0-0"; 300].join(","));
        assert_eq!(extract(Method::GET, Some(&many)), None);
    }
}

#[cfg(test)]
mod if_range {
    use http::HeaderValue;

    use crate::headers::{
        if_range::IfRange,
        range::{HttpRange, RangeSpec},
    };

    #[test]
    fn parse_date() {
        let ir: IfRange = "Sun, 05 Apr 2026 04:49:21 GMT".parse().unwrap();
        assert!(matches!(ir, IfRange::Date(_)));
    }

    #[test]
    fn parse_strong_etag() {
        let ir: IfRange = "\"abc123\"".parse().unwrap();
        assert!(matches!(ir, IfRange::ETag(_)));
    }

    #[test]
    fn parse_weak_etag() {
        let ir: IfRange = "W/\"abc123\"".parse().unwrap();
        assert!(matches!(ir, IfRange::ETag(_)));
    }

    #[test]
    fn empty_rejected() {
        assert!("".parse::<IfRange>().is_err());
    }

    #[test]
    fn whitespace_only_rejected() {
        assert!("   ".parse::<IfRange>().is_err());
    }

    #[test]
    fn date_matches_last_modified() {
        let ir: IfRange = "Sun, 05 Apr 2026 04:49:21 GMT".parse().unwrap();
        let lm = HeaderValue::from_static("Sun, 05 Apr 2026 04:49:21 GMT");
        let range = HttpRange::from(RangeSpec::StartingPoint(0));

        assert_eq!(ir.evaluate(range.clone(), Some(&lm), None), Some(range));
    }

    #[test]
    fn date_does_not_match_different_last_modified() {
        let ir: IfRange = "Mon, 01 Jan 2024 00:00:00 GMT".parse().unwrap();
        let lm = HeaderValue::from_static("Sun, 05 Apr 2026 04:49:21 GMT");
        let range = HttpRange::from(RangeSpec::StartingPoint(0));

        assert_eq!(ir.evaluate(range, Some(&lm), None), None);
    }

    #[test]
    fn date_does_not_match_missing_last_modified() {
        let ir: IfRange = "Sun, 05 Apr 2026 04:49:21 GMT".parse().unwrap();
        let range = HttpRange::from(RangeSpec::StartingPoint(0));

        assert_eq!(ir.evaluate(range, None, None), None);
    }

    #[test]
    fn strong_etag_matches() {
        let ir: IfRange = "\"abc123\"".parse().unwrap();
        let etag = HeaderValue::from_static("\"abc123\"");
        let range = HttpRange::from(RangeSpec::StartingPoint(0));

        assert_eq!(ir.evaluate(range.clone(), None, Some(&etag)), Some(range));
    }

    #[test]
    fn strong_etag_does_not_match_different() {
        let ir: IfRange = "\"abc123\"".parse().unwrap();
        let etag = HeaderValue::from_static("\"xyz789\"");
        let range = HttpRange::from(RangeSpec::StartingPoint(0));

        assert_eq!(ir.evaluate(range, None, Some(&etag)), None);
    }

    #[test]
    fn strong_etag_does_not_match_missing() {
        let ir: IfRange = "\"abc123\"".parse().unwrap();
        let range = HttpRange::from(RangeSpec::StartingPoint(0));

        assert_eq!(ir.evaluate(range, None, None), None);
    }

    #[test]
    fn weak_etag_never_matches_in_strong_comparison() {
        let ir: IfRange = "W/\"abc123\"".parse().unwrap();
        let etag = HeaderValue::from_static("W/\"abc123\"");
        let range = HttpRange::from(RangeSpec::StartingPoint(0));

        assert_eq!(ir.evaluate(range, None, Some(&etag)), None);
    }

    #[test]
    fn weak_if_range_does_not_match_strong_etag() {
        let ir: IfRange = "W/\"abc123\"".parse().unwrap();
        let etag = HeaderValue::from_static("\"abc123\"");
        let range = HttpRange::from(RangeSpec::StartingPoint(0));

        assert_eq!(ir.evaluate(range, None, Some(&etag)), None);
    }

    #[test]
    fn strong_if_range_does_not_match_weak_etag() {
        let ir: IfRange = "\"abc123\"".parse().unwrap();
        let etag = HeaderValue::from_static("W/\"abc123\"");
        let range = HttpRange::from(RangeSpec::StartingPoint(0));

        assert_eq!(ir.evaluate(range, None, Some(&etag)), None);
    }

    #[test]
    fn date_ignores_etag() {
        let ir: IfRange = "Sun, 05 Apr 2026 04:49:21 GMT".parse().unwrap();
        let etag = HeaderValue::from_static("\"abc123\"");
        let range = HttpRange::from(RangeSpec::StartingPoint(0));

        // Date-based If-Range only checks Last-Modified, not ETag
        assert_eq!(ir.evaluate(range, None, Some(&etag)), None);
    }

    #[test]
    fn etag_ignores_last_modified() {
        let ir: IfRange = "\"abc123\"".parse().unwrap();
        let lm = HeaderValue::from_static("Sun, 05 Apr 2026 04:49:21 GMT");
        let range = HttpRange::from(RangeSpec::StartingPoint(0));

        // ETag-based If-Range only checks ETag, not Last-Modified
        assert_eq!(ir.evaluate(range, Some(&lm), None), None);
    }
}
