use deft_reversi_engine::train_api::{
    N_BOARD_SQUARES, N_PATTERNS, N_ROTATIONS, PATTERN_DEFINITIONS, PATTERN_TABLE_SIZES,
    SQUARE_TO_FEATURES, TOTAL_PATTERN_WEIGHTS,
};

const BOARD_SIZE: usize = 8;

pub fn run(args: &[String]) -> Result<(), String> {
    match args {
        [] => {
            print_summary();
            println!();
            for pattern_idx in 0..N_PATTERNS {
                print_pattern(pattern_idx)?;
                println!();
            }
            Ok(())
        }
        [cmd] if cmd == "summary" => {
            print_summary();
            Ok(())
        }
        [cmd] if cmd == "overlay" => {
            print_overlay();
            Ok(())
        }
        [cmd, pattern_idx] if cmd == "show" || cmd == "pattern" => {
            let pattern_idx = parse_pattern_idx(pattern_idx)?;
            print_pattern(pattern_idx)
        }
        [pattern_idx] => {
            let pattern_idx = parse_pattern_idx(pattern_idx)?;
            print_pattern(pattern_idx)
        }
        _ => Err(usage()),
    }
}

fn parse_pattern_idx(value: &str) -> Result<usize, String> {
    let pattern_idx = value
        .parse::<usize>()
        .map_err(|_| format!("invalid pattern index: {value}"))?;
    if pattern_idx >= N_PATTERNS {
        return Err(format!(
            "pattern index out of range: {pattern_idx} (expected 0..{})",
            N_PATTERNS - 1
        ));
    }
    Ok(pattern_idx)
}

pub fn usage() -> String {
    [
        "usage:",
        "  deft-reversi-train patterns",
        "  deft-reversi-train patterns summary",
        "  deft-reversi-train patterns overlay",
        "  deft-reversi-train patterns show <pattern-index>",
    ]
    .join("\n")
}

fn print_summary() {
    println!("eval pattern summary");
    println!("patterns: {N_PATTERNS}");
    println!("rotations per pattern: {N_ROTATIONS}");
    println!("total pattern weights per phase: {TOTAL_PATTERN_WEIGHTS}");
    println!();
    println!(" idx | squares | table size");
    println!("-----+---------+-----------");
    for (idx, pattern) in PATTERN_DEFINITIONS.iter().enumerate() {
        println!(
            "{idx:>4} | {:>7} | {:>10}",
            pattern.n_squares, PATTERN_TABLE_SIZES[idx]
        );
    }
}

fn print_pattern(pattern_idx: usize) -> Result<(), String> {
    if pattern_idx >= N_PATTERNS {
        return Err(format!(
            "pattern index out of range: {pattern_idx} (expected 0..{})",
            N_PATTERNS - 1
        ));
    }

    let pattern = &PATTERN_DEFINITIONS[pattern_idx];
    println!(
        "pattern {pattern_idx}: squares={} table={}",
        pattern.n_squares, PATTERN_TABLE_SIZES[pattern_idx]
    );

    for rotation in 0..N_ROTATIONS {
        println!();
        println!("rotation {rotation}");
        let mut cells = empty_cells();
        for order in 0..pattern.n_squares {
            let square = pattern.squares[rotation][order] as usize;
            let row = square / BOARD_SIZE;
            let col = square % BOARD_SIZE;
            cells[row][col] = format!("{order:>2} ");
        }
        print_board(&cells);
    }

    Ok(())
}

fn print_overlay() {
    println!("feature coverage overlay");
    println!("cell value = number of pattern rotations containing the square");
    println!();

    let mut cells = empty_cells();
    for square in 0..N_BOARD_SQUARES {
        let row = square / BOARD_SIZE;
        let col = square % BOARD_SIZE;
        cells[row][col] = format!("{:>2} ", SQUARE_TO_FEATURES[square].len);
    }
    print_board(&cells);
}

fn print_board(cells: &[[String; BOARD_SIZE]; BOARD_SIZE]) {
    println!("     A   B   C   D   E   F   G   H");
    println!("   +---+---+---+---+---+---+---+---+");
    for (row_idx, row) in cells.iter().enumerate() {
        print!(" {} |", row_idx + 1);
        for cell in row {
            print!("{cell}|");
        }
        println!();
        println!("   +---+---+---+---+---+---+---+---+");
    }
}

fn empty_cells() -> [[String; BOARD_SIZE]; BOARD_SIZE] {
    std::array::from_fn(|_| std::array::from_fn(|_| " . ".to_string()))
}
