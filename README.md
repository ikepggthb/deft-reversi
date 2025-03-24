# Deft Reversi
![image](https://github.com/user-attachments/assets/269bb110-41af-43a3-bac4-d9ad281f163b)

Deft Reversi is a strong Othello program.

- [Deft Reversi Web](https://az.recazbowl.net/deft-reversi-web/index.html)

  This is the implementation of the Othello UI part.

- Deft Reversi Engine

  This is the AI part of the Othello program.

## Deft Reversi Web

- Playing with AI

  [Deft Reversi Web](https://az.recazbowl.net/deft-reversi-web/index.html)

- About the Levels

  There are levels from 0 to 24. It is thought that most people cannot win even around level 3. Levels above 20 are not recommended because they are heavy.

- Build
  - Rust and wasm-pack are required.

    ```
    git clone https://github.com/ikepggthb/deft-reversi.git
    cd deft_reversi/deft-reversi-web
    wasm-pack build --target web
    ```

The Human opening list is based on the one used on the following website.
[オセロ定石一覧(250種以上) : レコちゃんも頑張ってるのに](https://uenon1.com/archives/11101657.html)

## Deft Reversi Engine

- Features
  - bitboard
  - negascout search (PVS)
  - Transposition table
  - Multi Prob Cut
  - Move ordering
    - Shallow searches using evaluation functions for move ordering
    - Fastest-First heuristic
      - Searching from moves that limit the opponent's legal moves
      - Used in the endgame
    - Iterative deepening
      - Storing best move in the transposition table for use in the next search
  - Evaluation function using machine learning (linear regression)
    - Used board patterns and the difference in the number of legal moves as features.

## License
This project is licensed under the [GNU General Public License v3.0](LICENSE).

For more details, please refer to the [LICENSE](LICENSE) file.
