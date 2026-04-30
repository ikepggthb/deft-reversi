pub fn position_str_to_num(s: &str) -> Result<u64, &'static str> {
    if s.len() != 2 {
        return Err("Invalid position string");
    }

    let mut chars = s.chars();

    let col = match chars.next() {
        Some(c) => match c.to_ascii_uppercase() {
            'A' => 0,
            'B' => 1,
            'C' => 2,
            'D' => 3,
            'E' => 4,
            'F' => 5,
            'G' => 6,
            'H' => 7,
            _ => return Err("Invalid column letter"),
        },
        _ => return Err("Invalid column letter"),
    };

    let row = match chars.next() {
        Some('1') => 0,
        Some('2') => 1,
        Some('3') => 2,
        Some('4') => 3,
        Some('5') => 4,
        Some('6') => 5,
        Some('7') => 6,
        Some('8') => 7,
        _ => return Err("Invalid row number"),
    };

    Ok(row * 8 + col)
}

pub fn position_num_to_str(pos: u64) -> Result<String, &'static str> {
    if pos >= 64 {
        return Err("Invalid bit position");
    }

    let col = (pos % 8) as u8;
    let row = (pos / 8) as u8;

    let col_char = match col {
        0 => 'A',
        1 => 'B',
        2 => 'C',
        3 => 'D',
        4 => 'E',
        5 => 'F',
        6 => 'G',
        7 => 'H',
        _ => return Err("Invalid column"),
    };

    let row_char = match row {
        0 => '1',
        1 => '2',
        2 => '3',
        3 => '4',
        4 => '5',
        5 => '6',
        6 => '7',
        7 => '8',
        _ => return Err("Invalid row"),
    };

    Ok(format!("{}{}", col_char, row_char))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn position_str_to_num_accepts_board_corners() {
        assert_eq!(position_str_to_num("A1"), Ok(0));
        assert_eq!(position_str_to_num("H8"), Ok(63));
    }

    #[test]
    fn position_str_to_num_accepts_lowercase_column() {
        assert_eq!(position_str_to_num("d3"), Ok(19));
    }

    #[test]
    fn position_str_to_num_rejects_invalid_input() {
        assert_eq!(position_str_to_num("I1"), Err("Invalid column letter"));
        assert_eq!(position_str_to_num("A9"), Err("Invalid row number"));
        assert_eq!(position_str_to_num("A10"), Err("Invalid position string"));
    }

    #[test]
    fn position_num_to_str_accepts_board_corners() {
        assert_eq!(position_num_to_str(0), Ok("A1".to_string()));
        assert_eq!(position_num_to_str(63), Ok("H8".to_string()));
    }

    #[test]
    fn position_num_to_str_rejects_out_of_board_position() {
        assert_eq!(position_num_to_str(64), Err("Invalid bit position"));
    }
}
