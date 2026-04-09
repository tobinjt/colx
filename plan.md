1. **Understand the problem**:
    The issue is that `all_columns.insert(0, &line)` inside the per-line parsing loop causes an O(N) shift of elements, where N is the number of elements in the vector. We want to avoid this by replacing it with a better approach.
2. **Design the improvement**:
    We can construct the `Vec` directly such that the first element is `&line`.
    Using `std::iter::once(&line.as_str())` chained with the `filter` iterator from `delimiter.split` can achieve this in O(1) space operations per element.
    ```rust
    let mut all_columns: Vec<&str> = std::iter::once(line.as_str())
        .chain(delimiter.split(&line).filter(|col| !col.is_empty()))
        .collect();
    ```
    This approach creates the iterator, puts `line` as the first element, and then chains the rest of the columns, avoiding any array shifts.
3. **Verify the change**:
    Ensure the program continues to pass the unit tests by running `cargo test`.
4. **Pre-commit checks**:
    Run formatting and linting.
5. **Submit**:
    Submit the changes with an appropriate title and description.
