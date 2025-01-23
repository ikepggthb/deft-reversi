def rotate_coordinates(coordinates, degree):
    """
    Rotate the coordinates on an 8x8 Othello board.
    
    :param coordinates: List of coordinates in the format [A1, B2, ...]
    :param degree: Degree of rotation (90, 180, 270)
    :return: List of rotated coordinates
    """
    # Mapping of columns and rows to indices
    col_to_index = {col: index for index, col in enumerate("ABCDEFGH")}
    index_to_col = {index: col for col, index in col_to_index.items()}
    
    # Convert input coordinates to (row, column) indices
    coord_indices = [(8 - int(coord[1]), col_to_index[coord[0]]) for coord in coordinates]

    # Rotate coordinates
    rotated_indices = []
    for row, col in coord_indices:
        if degree == 90:
            rotated_indices.append((col, 7 - row))
        elif degree == 180:
            rotated_indices.append((7 - row, 7 - col))
        elif degree == 270:
            rotated_indices.append((7 - col, row))
        else:
            raise ValueError("Invalid rotation degree. Must be 90, 180, or 270.")

    # Convert back to original format
    rotated_coords = [index_to_col[col] + str(8 - row) for row, col in rotated_indices]
    return rotated_coords

# Test the function with example coordinates
# test_coords = ["A1", "B1", "C1", "D1", "E1", "F1", "G1", "H1", "B2", "G2"]
# rotated_90 = rotate_coordinates(test_coords, 90)
# rotated_180 = rotate_coordinates(test_coords, 180)
# rotated_270 = rotate_coordinates(test_coords, 270)



def extract_x_coordinates(board_representation):
    """
    Extracts the coordinates of 'X' from a given board representation.

    :param board_representation: A string representation of the board.
    :return: List of coordinates where 'X' is located.
    """
    board_lines = board_representation.strip().split('\n')
    x_coordinates = []

    for row_index, row in enumerate(board_lines):
        for col_index, cell in enumerate(row):
            if cell == 'X':
                # Correcting the row number calculation
                col_letter = chr(col_index + ord('A'))
                row_number = row_index + 1
                x_coordinates.append(f"{col_letter}{row_number}")

    return x_coordinates

# Example board representation
board = """
.......X
......X.
.....X..
....X...
...X....
..X.....
.X......
X.......
"""

# Extracting the coordinates
coords = extract_x_coordinates(board)
rotated_90 = rotate_coordinates(coords, 90)
rotated_180 = rotate_coordinates(coords, 180)
rotated_270 = rotate_coordinates(coords, 270)




print(f"[{', '.join(coords)}],\n[{', '.join(rotated_90)}],\n[{', '.join(rotated_180)}],\n[{', '.join(rotated_270)}]")
