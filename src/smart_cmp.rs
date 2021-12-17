use std::cmp::Ordering;

// Compares two strings the way that Windows Explorer does - when both
// filenames have one or more digits in the same location, consider
// those digits to be part of a single token and compare them numerically
// e.g. "1-456" comes after "1-7" even though 4 comes before 7, since
// 456 is larger than 7.
pub fn smart_cmp(a: &str, b: &str) -> Ordering {
    if a == b {
        return Ordering::Equal;
    }

    let mut a_chars = a.chars().peekable();
    let mut b_chars = b.chars().peekable();
    loop {
        if let Some(a_next) = a_chars.next() {
            if let Some(b_next) = b_chars.next() {
                if char::is_numeric(a_next) && char::is_numeric(b_next) {
                    let mut a_num = (a_next as u8 - b'0') as i32;
                    let mut b_num = (b_next as u8 - b'0') as i32;
                    while let Some(&a_next) = a_chars.peek() {
                        if char::is_numeric(a_next) {
                            a_num *= 10;
                            a_num += (a_next as u8 - b'0') as i32;
                            a_chars.next();
                        } else {
                            break;
                        }
                    }
                    while let Some(&b_next) = b_chars.peek() {
                        if char::is_numeric(b_next) {
                            b_num *= 10;
                            b_num += (b_next as u8 - b'0') as i32;
                            b_chars.next();
                        } else {
                            break;
                        }
                    }
                    if a_num.cmp(&b_num) != Ordering::Equal {
                        return a_num.cmp(&b_num);
                    }
                } else {
                    if a_next.cmp(&b_next) != Ordering::Equal {
                        return a_next.cmp(&b_next);
                    }
                }
            } else {
                return Ordering::Greater;
            }
        } else {
            return Ordering::Less;
        }
    }
}

#[cfg(test)]
mod test {
    use super::*;

    #[test]
    fn test_smart_cmp() {
        assert_eq!(smart_cmp("ch10_p9.jpg", "ch2_p9.jpg"), Ordering::Greater);
        assert_eq!(smart_cmp("ch1_p8.jpg", "ch1_p9.jpg"), Ordering::Less);
        assert_eq!(smart_cmp("ch10_p9.jpg", "ch10_p9.jpg"), Ordering::Equal);
        assert_eq!(
            smart_cmp("c166 (v21) - p000.jpg", "c166 (v21) - p000x1.png"),
            Ordering::Less
        );
        assert_eq!(
            smart_cmp("c166 (v21) - p000x2.jpg", "c166 (v21) - p001.jpg"),
            Ordering::Less
        );
    }
}
