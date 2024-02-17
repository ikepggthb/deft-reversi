# Deft Reversi

Deft Reversi is a strong Othello program. It consists of the following two parts:

- [Deft Reversi Web](https://az.recazbowl.net/deft_web/)

  This is the implementation of the Othello UI part.

- Deft Reversi Engine

  This is the AI part of the Othello program.

## Deft Reversi Web

- Playing with AI

  [Deft Reversi Web](https://az.recazbowl.net/deft_web/)

- About the Levels

  There are levels from 0 to 22. Level 0 plays randomly. It is thought that most people cannot win even around level 3. Levels above 20 are not recommended because they are heavy.

- Build
  - Rust and wasm-pack are required.

    ```
    git clone https://github.com/ikepggthb/deft-reversi.git
    cd deft_reversi/deft-reversi-web
    wasm-pack build --target web
    ```

## Deft Reversi Engine

- Features
  - bitboard
  - negascout search (PVS)
  - Transposition table
  - Multi Prob Cut
  - Move ordering
    - Shallow searches using evaluation functions for move ordering
    - Speed priority search
      - Searching from moves that limit the opponent's legal moves
      - Used in the endgame
    - Iterative deepening
      - Storing best move in the transposition table for use in the next search
  - Evaluation function using machine learning (linear regression)
    - Used board patterns and the difference in the number of legal moves as features.

## License
[MIT License](https://opensource.org/license/mit/).