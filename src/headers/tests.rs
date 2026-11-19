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

#[cfg(feature = "multipart")]
mod file_range_multipart {
    use std::ops::Range;

    use crate::{
        FileRanges, file_range_multipart,
        headers::{
            content_range::{Bound, HttpContentRange, Unsatisfiable},
            range::{HttpRange, MAX_RANGE_SET_ELEMENTS},
        },
    };

    fn parsed(s: &str) -> Option<HttpRange> {
        Some(s.parse().unwrap())
    }

    fn assert_part_header(size: u64, header: Option<HttpContentRange>, range: &Range<u64>) {
        assert_eq!(
            header,
            Some(HttpContentRange::Bound(
                Bound::new(range.start..=range.end - 1, Some(size)).unwrap()
            ))
        );
    }

    fn single(size: u64, s: &str) -> Range<u64> {
        let FileRanges::Single(part) = file_range_multipart(size, parsed(s)).unwrap() else {
            panic!("expected a single part response to {s:?}");
        };
        assert_part_header(size, part.header(), part.range());
        part.range().clone()
    }

    fn multipart(size: u64, s: &str) -> Vec<Range<u64>> {
        let FileRanges::Multipart(parts) = file_range_multipart(size, parsed(s)).unwrap() else {
            panic!("expected a multipart response to {s:?}");
        };
        parts
            .iter()
            .map(|part| {
                assert_part_header(size, part.header(), part.range());
                part.range().clone()
            })
            .collect()
    }

    #[test]
    fn no_range_is_full() {
        assert_eq!(file_range_multipart(10, None).unwrap(), FileRanges::Full);
    }

    #[test]
    fn single_range_is_never_multipart() {
        assert_eq!(single(10, "bytes=0-"), 0..10);
        assert_eq!(single(10, "bytes=2-3"), 2..4);
        assert_eq!(single(10, "bytes=-1"), 9..10);
    }

    #[test]
    fn ranges_coalescing_into_one_are_a_single_part() {
        assert_eq!(single(100, "bytes=0-50,30-80"), 0..81);
        assert_eq!(single(1000, "bytes=0-0,37-37"), 0..38);
        assert_eq!(single(10, "bytes=50-60,2-3"), 2..4);
    }

    #[test]
    fn disjoint_ranges_are_multipart() {
        // The example of RFC 9110 Section 14.1.2.
        assert_eq!(multipart(10000, "bytes=0-0,-1"), [0..1, 9999..10000]);
        assert_eq!(
            multipart(10000, "bytes= 0-999, 4500-5499, -1000"),
            [0..1000, 4500..5500, 9000..10000]
        );
        // The gap is as large as the overhead of one more part.
        assert_eq!(multipart(100, "bytes=0-0,38-38"), [0..1, 38..39]);
    }

    #[test]
    fn multipart_keeps_request_order() {
        assert_eq!(multipart(100, "bytes=50-59,0-9"), [50..60, 0..10]);
        assert_eq!(
            multipart(1000, "bytes=-10,500-599,0-9"),
            [990..1000, 500..600, 0..10]
        );
    }

    #[test]
    fn multipart_coalesced_range_takes_the_place_of_its_first_range() {
        // 0-9 and 5-14 are coalesced into 0-14, in the place of 5-14.
        assert_eq!(
            multipart(1000, "bytes=500-599,5-14,900-,0-9"),
            [500..600, 0..15, 900..1000]
        );
    }

    #[test]
    fn multipart_drops_unsatisfiable_ranges() {
        assert_eq!(
            multipart(100, "bytes=-0,0-9,100-,50-59,200-300"),
            [0..10, 50..60]
        );
    }

    fn small_ranges(count: usize) -> String {
        let ranges = (0..count)
            .rev()
            .map(|i| format!("{0}-{0}", i * 100))
            .collect::<Vec<_>>()
            .join(",");
        format!("bytes={ranges}")
    }

    #[test]
    fn up_to_32_small_ranges_are_multipart() {
        let parts = multipart(100 * 32, &small_ranges(32));
        assert_eq!(parts.len(), 32);
        assert_eq!(parts[0], 3100..3101);
    }

    #[test]
    fn more_than_32_small_ranges_are_ignored() {
        for count in [33, MAX_RANGE_SET_ELEMENTS] {
            let result = file_range_multipart(100 * count as u64, parsed(&small_ranges(count)));
            assert_eq!(result.unwrap(), FileRanges::Full);
        }
    }

    #[test]
    fn many_ranges_coalescing_into_few_parts_are_multipart() {
        let ranges = (0..MAX_RANGE_SET_ELEMENTS - 1)
            .map(|i| format!("{}-{}", i * 10, i * 10 + 9))
            .collect::<Vec<_>>()
            .join(",");
        let parts = multipart(10_000, &format!("bytes={ranges},9000-"));
        assert_eq!(parts, [0..2550, 9000..10_000]);
    }

    #[test]
    fn unsatisfiable_ranges_are_unsatisfiable() {
        for (size, s) in [(10, "bytes=10-,20-30,-0"), (0, "bytes=0-1,-0,0-")] {
            let err = file_range_multipart(size, parsed(s)).unwrap_err();
            assert_eq!(
                err.header(),
                HttpContentRange::Unsatisfiable(Unsatisfiable::new(size))
            );
        }
    }

    #[test]
    fn size_zero_satisfiable_ranges_are_full() {
        let result = file_range_multipart(0, parsed("bytes=0-1,-5")).unwrap();
        assert_eq!(result, FileRanges::Full);
    }
}

#[cfg(feature = "multipart")]
mod serve_file_multipart {
    use bytes::Bytes;
    use http::{HeaderValue, StatusCode};

    use crate::{
        MultipartBodyRange,
        headers::{
            content_range::{Bound, HttpContentRange, Unsatisfiable},
            range::HttpRange,
        },
        multipart::tests::{collect, is_valid_boundary},
        serve_file_with_http_range_multipart,
    };

    fn parsed(s: &str) -> Option<HttpRange> {
        Some(s.parse().unwrap())
    }

    fn content_type() -> Option<HeaderValue> {
        Some(HeaderValue::from_static("text/plain; charset=utf-8"))
    }

    fn serve(
        body: &Bytes,
        range: Option<&str>,
        content_type: Option<HeaderValue>,
    ) -> MultipartBodyRange {
        serve_file_with_http_range_multipart(body.clone(), range.and_then(parsed), content_type)
            .unwrap()
    }

    fn boundary(body_range: &MultipartBodyRange) -> String {
        let content_type = body_range.content_type().unwrap().to_str().unwrap();
        let boundary = content_type
            .strip_prefix("multipart/byteranges; boundary=")
            .unwrap();
        assert!(is_valid_boundary(boundary));
        boundary.to_owned()
    }

    /// Splits `multipart/byteranges` content into the headers and the
    /// content of each part.
    fn parse_multipart(body_range: MultipartBodyRange) -> Vec<(String, Bytes)> {
        let boundary = boundary(&body_range);
        let body = Bytes::from(collect(body_range.into_body()).1);
        let close_delimiter = format!("\r\n--{boundary}--\r\n");
        assert!(body.ends_with(close_delimiter.as_bytes()));
        let body = body.slice(..body.len() - close_delimiter.len());
        let dash_boundary = format!("--{boundary}\r\n");
        assert!(body.starts_with(dash_boundary.as_bytes()));
        let delimiter = format!("\r\n--{boundary}\r\n");

        let mut parts = Vec::new();
        let mut rest = body.slice(dash_boundary.len()..);
        loop {
            let end = rest
                .windows(delimiter.len())
                .position(|window| window == delimiter.as_bytes())
                .unwrap_or(rest.len());
            let part = rest.slice(..end);
            let headers_end = part.windows(4).position(|w| w == b"\r\n\r\n").unwrap();
            let headers = String::from_utf8(part[..headers_end + 2].to_vec()).unwrap();
            parts.push((headers, part.slice(headers_end + 4..)));
            if end == rest.len() {
                return parts;
            }
            rest = rest.slice(end + delimiter.len()..);
        }
    }

    #[test]
    fn full_body() {
        let body = Bytes::from_static(b"hello world");
        let result = serve(&body, None, content_type());
        assert_eq!(result.status(), StatusCode::OK);
        assert_eq!(result.content_type(), content_type().as_ref());
        assert!(result.header().is_none());
        let (frames, _) = collect(result.into_body());
        assert_eq!(frames, [body]);
    }

    #[test]
    fn single_part_does_not_copy_the_body() {
        let body = Bytes::from_static(b"hello world");
        let result = serve(&body, Some("bytes=-5,6-7"), content_type());
        assert_eq!(result.status(), StatusCode::PARTIAL_CONTENT);
        assert_eq!(result.content_type(), content_type().as_ref());
        assert_eq!(
            result.header(),
            Some(HttpContentRange::Bound(
                Bound::new(6..=10, Some(11)).unwrap()
            ))
        );
        let (frames, _) = collect(result.into_body());
        assert_eq!(frames, [Bytes::from_static(b"world")]);
        assert_eq!(frames[0].as_ptr(), body[6..].as_ptr());
    }

    #[test]
    fn multiple_ranges_are_multipart() {
        let mut body = vec![b'x'; 200];
        body[..5].copy_from_slice(b"hello");
        body[195..].copy_from_slice(b"world");
        let result = serve(&Bytes::from(body), Some("bytes=-5,0-4"), content_type());

        assert_eq!(result.status(), StatusCode::PARTIAL_CONTENT);
        assert!(result.header().is_none());
        let boundary = boundary(&result);
        let expected = format!(
            "--{boundary}\r\n\
             Content-Type: text/plain; charset=utf-8\r\n\
             Content-Range: bytes 195-199/200\r\n\
             \r\n\
             world\r\n\
             --{boundary}\r\n\
             Content-Type: text/plain; charset=utf-8\r\n\
             Content-Range: bytes 0-4/200\r\n\
             \r\n\
             hello\r\n\
             --{boundary}--\r\n"
        );
        assert_eq!(collect(result.into_body()).1, expected.as_bytes());
    }

    #[test]
    fn multipart_boundaries_differ() {
        let body = Bytes::from(vec![b'x'; 200]);
        let first = serve(&body, Some("bytes=0-0,-1"), None);
        let second = serve(&body, Some("bytes=0-0,-1"), None);
        assert_ne!(boundary(&first), boundary(&second));
    }

    #[test]
    fn multipart_without_content_type() {
        let body = Bytes::from((0..=255).collect::<Vec<u8>>());
        let result = serve(&body, Some("bytes=0-1,100-101"), None);

        assert_eq!(
            parse_multipart(result),
            [
                (
                    "Content-Range: bytes 0-1/256\r\n".to_owned(),
                    Bytes::from_static(&[0, 1])
                ),
                (
                    "Content-Range: bytes 100-101/256\r\n".to_owned(),
                    Bytes::from_static(&[100, 101])
                ),
            ]
        );
    }

    #[test]
    fn multipart_parts_with_crlf_and_dashes() {
        let mut body = vec![b'.'; 300];
        body[..10].copy_from_slice(b"\r\n--\r\n--\r\n");
        body[290..].copy_from_slice(b"--\r\n\r\n--\r\n");
        let body = Bytes::from(body);
        let parts = parse_multipart(serve(&body, Some("bytes=0-9,-10"), content_type()));

        assert_eq!(parts.len(), 2);
        assert_eq!(parts[0].1, body.slice(..10));
        assert_eq!(parts[1].1, body.slice(290..));
    }

    #[test]
    fn multipart_with_overlapping_and_unsatisfiable_ranges() {
        let body = Bytes::from(vec![b'a'; 1000]);
        let result = serve(
            &body,
            Some("bytes=900-949,2000-,0-9,5-14,950-999,-0"),
            content_type(),
        );

        let ranges = parse_multipart(result)
            .into_iter()
            .map(|(headers, content)| {
                assert!(headers.starts_with("Content-Type: text/plain; charset=utf-8\r\n"));
                (headers.lines().nth(1).unwrap().to_owned(), content.len())
            })
            .collect::<Vec<_>>();
        assert_eq!(
            ranges,
            [
                ("Content-Range: bytes 900-999/1000".to_owned(), 100),
                ("Content-Range: bytes 0-14/1000".to_owned(), 15),
            ]
        );
    }

    #[test]
    fn multipart_does_not_copy_the_body() {
        let body = Bytes::from(vec![b'a'; 64 << 20]);
        let result = serve(&body, Some("bytes=0-0,100-"), content_type());

        let (frames, _) = collect(result.into_body());
        assert_eq!(frames.len(), 5);
        assert_eq!(frames[1].as_ptr(), body.as_ptr());
        assert_eq!(frames[3].as_ptr(), body[100..].as_ptr());
        assert_eq!(frames[3].len(), body.len() - 100);
        assert!(frames[0].len() + frames[2].len() + frames[4].len() < 1024);
    }

    #[test]
    fn multipart_length_is_exact_for_any_number_of_parts() {
        let body = Bytes::from(vec![b'a'; 100_000]);
        for count in 2..=32 {
            let ranges = (0..count)
                .map(|i| format!("{}-{}", i * 3001, i * 3001 + i))
                .collect::<Vec<_>>()
                .join(",");
            let result = serve(&body, Some(&format!("bytes={ranges}")), content_type());
            let parts = parse_multipart(result);
            assert_eq!(parts.len(), count);
        }
    }

    #[test]
    fn multiple_unsatisfiable_ranges_are_unsatisfiable() {
        let body = Bytes::from_static(b"hello");
        let result =
            serve_file_with_http_range_multipart(body, parsed("bytes=5-,10-20,-0"), content_type());
        assert_eq!(
            result.unwrap_err().header(),
            HttpContentRange::Unsatisfiable(Unsatisfiable::new(5))
        );
    }

    #[test]
    fn empty_body_with_multiple_ranges() {
        let result = serve(&Bytes::new(), Some("bytes=0-1,-5"), content_type());
        assert_eq!(result.status(), StatusCode::OK);
        assert!(collect(result.into_body()).0.is_empty());

        let result =
            serve_file_with_http_range_multipart(Bytes::new(), parsed("bytes=0-1,0-"), None);
        assert!(result.is_err());
    }
}

#[cfg(all(feature = "axum", feature = "multipart"))]
mod axum_multipart_response {
    use axum_core::{body::Body, response::IntoResponse};
    use bytes::Bytes;
    use http::{
        HeaderValue, StatusCode,
        header::{CONTENT_RANGE, CONTENT_TYPE},
    };

    use crate::{multipart::tests::collect, serve_file_with_http_range_multipart};

    fn respond_with(
        range: Option<&str>,
        content_type: Option<HeaderValue>,
    ) -> http::Response<Body> {
        let body = Bytes::from(vec![b'a'; 100]);
        let range = range.map(|r| r.parse().unwrap());
        match serve_file_with_http_range_multipart(body, range, content_type) {
            Ok(body_range) => body_range.into_response(),
            Err(unsatisfiable) => unsatisfiable.into_response(),
        }
    }

    fn respond(range: Option<&str>) -> http::Response<Body> {
        respond_with(range, Some(HeaderValue::from_static("text/plain")))
    }

    fn body(response: http::Response<Body>) -> Vec<u8> {
        collect(response.into_body()).1
    }

    #[test]
    fn full_response() {
        let response = respond(None);
        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(response.headers()[CONTENT_TYPE], "text/plain");
        assert!(response.headers().get(CONTENT_RANGE).is_none());
        assert_eq!(body(response), [b'a'; 100]);
    }

    #[test]
    fn single_part_response() {
        let response = respond(Some("bytes=0-9,5-19"));
        assert_eq!(response.status(), StatusCode::PARTIAL_CONTENT);
        assert_eq!(response.headers()[CONTENT_TYPE], "text/plain");
        assert_eq!(response.headers()[CONTENT_RANGE], "bytes 0-19/100");
        assert_eq!(body(response), [b'a'; 20]);
    }

    #[test]
    fn multipart_response() {
        let response = respond(Some("bytes=0-0,-1"));
        assert_eq!(response.status(), StatusCode::PARTIAL_CONTENT);
        assert!(response.headers().get(CONTENT_RANGE).is_none());
        let boundary = response.headers()[CONTENT_TYPE]
            .to_str()
            .unwrap()
            .strip_prefix("multipart/byteranges; boundary=")
            .unwrap()
            .to_owned();

        assert_eq!(
            body(response),
            format!(
                "--{boundary}\r\n\
                 Content-Type: text/plain\r\n\
                 Content-Range: bytes 0-0/100\r\n\
                 \r\n\
                 a\r\n\
                 --{boundary}\r\n\
                 Content-Type: text/plain\r\n\
                 Content-Range: bytes 99-99/100\r\n\
                 \r\n\
                 a\r\n\
                 --{boundary}--\r\n"
            )
            .as_bytes()
        );
    }

    #[test]
    fn unknown_content_type_is_never_sent() {
        for range in [None, Some("bytes=0-9")] {
            let response = respond_with(range, None);
            assert!(response.headers().get(CONTENT_TYPE).is_none(), "{range:?}");
        }

        let response = respond_with(Some("bytes=0-0,-1"), None);
        assert!(
            response.headers()[CONTENT_TYPE]
                .to_str()
                .unwrap()
                .starts_with("multipart/byteranges; boundary=")
        );
        assert!(!body(response).windows(13).any(|w| w == b"Content-Type:"));
    }

    #[test]
    fn unsatisfiable_response() {
        let response = respond(Some("bytes=100-,-0"));
        assert_eq!(response.status(), StatusCode::RANGE_NOT_SATISFIABLE);
        assert_eq!(response.headers()[CONTENT_RANGE], "bytes */100");
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
