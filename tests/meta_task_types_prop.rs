//! Properties of the typed meta-task tracking values.
//!
//! `DueDate` has to accept exactly the days the Gregorian calendar has, print
//! back the text it parsed, and order as dates order. `MetaTaskHandle` has to
//! accept both of its textual forms and hand back the number it was built from.

use covey::{DueDate, MetaTaskHandle};
use proptest::prelude::*;

fn gregorian_days_in_month(year: u32, month: u32) -> u32 {
    let leap = year.is_multiple_of(4) && (!year.is_multiple_of(100) || year.is_multiple_of(400));
    match month {
        1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
        4 | 6 | 9 | 11 => 30,
        2 if leap => 29,
        2 => 28,
        _ => 0,
    }
}

fn valid_date() -> impl Strategy<Value = (DueDate, (u32, u32, u32))> {
    (0_u32..10_000, 1_u32..13, 1_u32..32).prop_filter_map(
        "a day that exists in the generated month",
        |(year, month, day)| {
            if day > gregorian_days_in_month(year, month) {
                return None;
            }
            let text = format!("{year:04}-{month:02}-{day:02}");
            let date = DueDate::parse(text).ok()?;
            Some((date, (year, month, day)))
        },
    )
}

proptest! {
    #[test]
    fn due_date_parses_exactly_the_days_of_its_month(
        year in 0_u32..10_000,
        month in 1_u32..13,
        day in 1_u32..32,
    ) {
        let text = format!("{year:04}-{month:02}-{day:02}");
        let parsed = DueDate::parse(text.as_str());
        prop_assert_eq!(parsed.is_ok(), day <= gregorian_days_in_month(year, month));
        if let Ok(date) = parsed {
            prop_assert_eq!(date.to_string(), text);
        }
    }

    #[test]
    fn due_date_orders_as_the_calendar_orders(a in valid_date(), b in valid_date()) {
        let (left, left_parts) = a;
        let (right, right_parts) = b;
        prop_assert_eq!(left.cmp(&right), left_parts.cmp(&right_parts));
    }

    #[test]
    fn meta_task_handle_parses_plain_and_hash_forms(
        number in any::<u64>().prop_filter("a positive handle", |number| *number >= 1),
    ) {
        let digits = number.to_string();
        let plain = MetaTaskHandle::parse(digits.as_str()).expect("plain digits name a handle");
        prop_assert_eq!(plain.get(), number);
        let prefixed = MetaTaskHandle::parse(format!("#{digits}")).expect("#digits name a handle");
        prop_assert_eq!(prefixed.get(), number);
        prop_assert_eq!(plain, prefixed);
    }
}
