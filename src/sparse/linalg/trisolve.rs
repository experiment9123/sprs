/// Sparse triangular solves

use std::ops::IndexMut;
use num::traits::Num;
use sparse::CsMatView;
use sparse::vec::{self, VecDim};
use errors::SprsError;
use stack::{self, StackVal, DStack};

fn check_solver_dimensions<N, V: ?Sized>(lower_tri_mat: &CsMatView<N>, rhs: &V)
where N: Copy + Num,
      V: vec::VecDim<N>
{
    let (cols, rows) = (lower_tri_mat.cols(), lower_tri_mat.rows());
    if cols != rows {
        panic!("Non square matrix passed to solver");
    }
    if cols != rhs.dim() {
        panic!("Dimension mismatch");
    }
}

/// Solve a sparse lower triangular matrix system, with a csr matrix
/// and a dense vector as inputs
///
/// The solve results are written into the provided values.
///
/// This solve does not assume the input matrix to actually be
/// triangular, instead it ignores the upper triangular part.
pub fn lsolve_csr_dense_rhs<N, V: ?Sized>(lower_tri_mat: CsMatView<N>,
                                          rhs: &mut V)
                                          -> Result<(), SprsError>
where N: Copy + Num,
      V: IndexMut<usize, Output = N> + vec::VecDim<N>
{
    check_solver_dimensions(&lower_tri_mat, rhs);
    if !lower_tri_mat.is_csr() {
        panic!("Storage mismatch");
    }

    // we base our algorithm on the following decomposition:
    // | L_0_0    0     | | x_0 |    | b_0 |
    // | l_1_0^T  l_1_1 | | x_1 |  = | b_1 |
    //
    // At each step of the algorithm, the x_0 part is known,
    // and x_1 can be computed as x_1 = (b_1 - l_1_0^T.x_0) / l_1_1

    for (row_ind, row) in lower_tri_mat.outer_iterator().enumerate() {
        let mut diag_val = N::zero();
        let mut x = rhs[row_ind];
        for (col_ind, &val) in row.iter() {
            if col_ind == row_ind {
                diag_val = val;
                continue;
            }
            if col_ind > row_ind {
                continue;
            }
            x = x - val * rhs[col_ind];
        }
        if diag_val == N::zero() {
            return Err(SprsError::SingularMatrix);
        }
        rhs[row_ind] = x / diag_val;
    }
    Ok(())
}


/// Solve a sparse lower triangular matrix system, with a csc matrix
/// and a dense vector as inputs
///
/// The solve results are written into the provided values.
///
/// This method does not require the matrix to actually be lower triangular,
/// but is most efficient if the first element of each column
/// is the diagonal element (thus actual sorted lower triangular matrices work
/// best). Otherwise, logarithmic search for the diagonal element
/// has to be performed for each column.
pub fn lsolve_csc_dense_rhs<N, V: ?Sized>(lower_tri_mat: CsMatView<N>,
                                          rhs: &mut V)
                                          -> Result<(), SprsError>
where N: Copy + Num,
      V: IndexMut<usize, Output = N> + vec::VecDim<N>
{
    check_solver_dimensions(&lower_tri_mat, rhs);
    if !lower_tri_mat.is_csc() {
        panic!("Storage mismatch");
    }

    // we base our algorithm on the following decomposition:
    // |l_0_0    0    | |x_0|    |b_0|
    // |l_1_0    L_1_1| |x_1|  = |b_1|
    //
    // At each step of the algorithm, the x_0 part is computed as b_0 / l_0_0
    // and the step can be propagated by solving the reduced system
    // L_1_1 x1 = b_1 - x0*l_1_0

    for (col_ind, col) in lower_tri_mat.outer_iterator().enumerate() {
        try!(lspsolve_csc_process_col(col, col_ind, rhs));
    }
    Ok(())
}

fn lspsolve_csc_process_col<N: Copy + Num, V: ?Sized>
                                                      (col: vec::CsVecView<N>,
                                                       col_ind: usize,
                                                       rhs: &mut V)
                                                       -> Result<(), SprsError>
where V: vec::VecDim<N> + IndexMut<usize, Output = N>
{
    if let Some(&diag_val) = col.get(col_ind) {
        if diag_val == N::zero() {
            return Err(SprsError::SingularMatrix);
        }
        let b = rhs[col_ind];
        let x = b / diag_val;
        rhs[col_ind] = x;
        for (row_ind, &val) in col.iter() {
            if row_ind <= col_ind {
                continue;
            }
            let b = rhs[row_ind];
            rhs[row_ind] = b - val * x;
        }
    } else {
        return Err(SprsError::SingularMatrix);
    }
    Ok(())
}

/// Solve a sparse upper triangular matrix system, with a csc matrix
/// and a dense vector as inputs
///
/// The solve results are written into the provided values.
///
/// This method does not require the matrix to actually be lower triangular,
/// but is most efficient if the last element of each column
/// is the diagonal element (thus actual sorted lower triangular matrices work
/// best). Otherwise, logarithmic search for the diagonal element
/// has to be performed for each column.
pub fn usolve_csc_dense_rhs<N, V: ?Sized>(upper_tri_mat: CsMatView<N>,
                                          rhs: &mut V)
                                          -> Result<(), SprsError>
where N: Copy + Num,
      V: IndexMut<usize, Output = N> + vec::VecDim<N>
{
    check_solver_dimensions(&upper_tri_mat, rhs);
    if !upper_tri_mat.is_csc() {
        panic!("Storage mismatch");
    }

    // we base our algorithm on the following decomposition:
    // | U_0_0    u_0_1 | | x_0 |    | b_0 |
    // |   0      u_1_1 | | x_1 |  = | b_1 |
    //
    // At each step of the algorithm, the x_1 part is computed as b_1 / u_1_1
    // and the step can be propagated by solving the reduced system
    // U_0_0 x0 = b_0 - x1*u_0_1

    for (col_ind, col) in upper_tri_mat.outer_iterator().enumerate().rev() {
        if let Some(&diag_val) = col.get(col_ind) {
            if diag_val == N::zero() {
                return Err(SprsError::SingularMatrix);
            }
            let b = rhs[col_ind];
            let x = b / diag_val;
            rhs[col_ind] = x;
            for (row_ind, &val) in col.iter() {
                if row_ind >= col_ind {
                    continue;
                }
                let b = rhs[row_ind];
                rhs[row_ind] = b - val * x;
            }
        } else {
            return Err(SprsError::SingularMatrix);
        }
    }

    Ok(())
}

/// Solve a sparse lower triangular matrix system, with a csr matrix
/// and a dense vector as inputs
///
/// The solve results are written into the provided values.
///
/// This solve does not assume the input matrix to actually be
/// triangular, instead it ignores the upper triangular part.
pub fn usolve_csr_dense_rhs<N, V: ?Sized>(upper_tri_mat: CsMatView<N>,
                                          rhs: &mut V)
                                          -> Result<(), SprsError>
where N: Copy + Num,
      V: IndexMut<usize, Output = N> + vec::VecDim<N>
{
    check_solver_dimensions(&upper_tri_mat, rhs);
    if !upper_tri_mat.is_csr() {
        panic!("Storage mismatch");
    }
    // we base our algorithm on the following decomposition:
    // | u_0_0    u_0_1^T | | x_0 |    | b_0 |
    // |   0      U_1_1   | | x_1 |  = | b_1 |
    //
    // At each step of the algorithm, the x_1 part is known from previous
    // iterations and x_0 can be computed as
    // x0 = (b_0 - u_0_1^T.x_1) / u_0_0
    for (row_ind, row) in upper_tri_mat.outer_iterator().enumerate().rev() {
        let mut diag_val = N::zero();
        let mut x = rhs[row_ind];
        for (col_ind, &val) in row.iter() {
            if col_ind == row_ind {
                diag_val = val;
                continue;
            }
            if col_ind < row_ind {
                continue;
            }
            x = x - val * rhs[col_ind];
        }
        if diag_val == N::zero() {
            return Err(SprsError::SingularMatrix);
        }
        rhs[row_ind] = x / diag_val;
    }
    Ok(())
}

/// Sparse triangular CSC / sparse vector solve
///
/// lower_tri_mat is a sparse lower triangular matrix of shape (n, n)
/// rhs is a sparse vector of size n
/// dstack is a double stack with capacity 2*n
/// x_workspace is a workspace vector with length equal to the number of
/// rows of lower_tri_mat. Its input values can be anything.
/// visited is a workspace vector of same size as upper_tri_mat.indptr(),
/// and should be all false.
///
/// On succesful execution, dstack will hold the non-zero pattern in its
/// right stack, and x_workspace will contain the solve values at the indices
/// contained in right stack. The non-zero pattern indices are not guaranteed
/// to be sorted (they are sorted for each connected component of the matrix's
/// graph).
///
/// # Panics
///
/// * if dstack.capacity() is too small
/// * if dstack is not empty
/// * if w_workspace is not of length n
///
pub fn lsolve_csc_sparse_rhs<N>(lower_tri_mat: CsMatView<N>,
                                rhs: vec::CsVecView<N>,
                                dstack: &mut DStack<StackVal<usize>>,
                                x_workspace: &mut [N],
                                visited: &mut [bool])
                                -> Result<(), SprsError>
where N: Copy + Num
{
    if !lower_tri_mat.is_csc() {
        return Err(SprsError::BadStorageType);
    }
    let n = lower_tri_mat.rows();
    assert!(dstack.capacity() >= 2 * n, "dstack cap should be 2*n");
    assert!(dstack.is_left_empty() && dstack.is_right_empty(),
            "dstack should be empty");
    assert!(x_workspace.len() == n, "x should be of len n");

    // the solve works out the sparsity of the solution using depth first
    // search on the matrix's graph
    // |0              | |   |     |   |
    // |  1            | | x |     | a |     x = a / l1
    // |    2          | |   |     |   |
    // |      3        | |   |     |   |
    // |  d     4      | | y |  =  | b |     x*d + l4*y = b
    // |          5    | |   |     |   |
    // |        e   6  | | z |     |   |     y*e + l6*z = 0
    // |      f       7| | w |     | c |     w = c / l7

    // compute the non-zero elements of the result by dfs traversal
    for (root_ind, _) in rhs.iter() {
        if visited[root_ind] {
            continue;
        }
        dstack.push_left(StackVal::Enter(root_ind));
        while let Some(stack_val) = dstack.pop_left() {
            match stack_val {
                StackVal::Enter(ind) => {
                    if visited[ind] {
                        continue;
                    }
                    visited[ind] = true;
                    dstack.push_left(StackVal::Exit(ind));
                    if let Some(column) = lower_tri_mat.outer_view(ind) {
                        for (child_ind, _) in column.iter() {
                            dstack.push_left(StackVal::Enter(child_ind));
                        }
                    } else {
                        unreachable!();
                    }
                }
                StackVal::Exit(ind) => {
                    dstack.push_right(StackVal::Enter(ind));
                }
            }
        }
    }

    // solve for the non-zero values into dense workspace
    rhs.scatter(x_workspace);
    for &ind in dstack.iter_right().map(stack::extract_stack_val) {
        println!("ind: {}", ind);
        let col = lower_tri_mat.outer_view(ind).expect("ind not in bounds");
        try!(lspsolve_csc_process_col(col, ind, x_workspace));
    }

    Ok(())
}

#[cfg(test)]
mod test {

    use sparse::{CsMatOwned, vec};
    use stack::{self, DStack};
    use std::collections::HashSet;

    #[test]
    fn lsolve_csr_dense_rhs() {
        // |1    | |3|   |3|
        // |0 2  | |1| = |2|
        // |1 0 1| |1|   |4|
        let l = CsMatOwned::new((3, 3),
                                vec![0, 1, 2, 4],
                                vec![0, 1, 0, 2],
                                vec![1, 2, 1, 1]);
        let b = vec![3, 2, 4];
        let mut x = b.clone();

        super::lsolve_csr_dense_rhs(l.view(), &mut x).unwrap();
        assert_eq!(x, vec![3, 1, 1]);
    }

    #[test]
    fn lsolve_csc_dense_rhs() {
        // |1    | |3|   |3|
        // |1 2  | |1| = |5|
        // |0 0 3| |1|   |3|
        let l = CsMatOwned::new_csc((3, 3),
                                    vec![0, 2, 3, 4],
                                    vec![0, 1, 1, 2],
                                    vec![1, 1, 2, 3]);
        let b = vec![3, 5, 3];
        let mut x = b.clone();

        super::lsolve_csc_dense_rhs(l.view(), &mut x).unwrap();
        assert_eq!(x, vec![3, 1, 1]);
    }

    #[test]
    fn usolve_csc_dense_rhs() {
        // |1 0 1| |3|   |4|
        // |  2 0| |1| = |2|
        // |    3| |1|   |3|
        let u = CsMatOwned::new_csc((3, 3),
                                    vec![0, 1, 2, 4],
                                    vec![0, 1, 0, 2],
                                    vec![1, 2, 1, 3]);
        let b = vec![4, 2, 3];
        let mut x = b.clone();

        super::usolve_csc_dense_rhs(u.view(), &mut x).unwrap();
        assert_eq!(x, vec![3, 1, 1]);
    }

    #[test]
    fn usolve_csr_dense_rhs() {
        // |1 1 0| |3|   |4|
        // |  5 3| |1| = |8|
        // |    1| |1|   |1|
        let u = CsMatOwned::new((3, 3),
                                vec![0, 2, 4, 5],
                                vec![0, 1, 1, 2, 2],
                                vec![1, 1, 5, 3, 1]);
        let b = vec![4, 8, 1];
        let mut x = b.clone();

        super::usolve_csr_dense_rhs(u.view(), &mut x).unwrap();
        assert_eq!(x, vec![3, 1, 1]);
    }

    #[test]
    fn lspsolve_csc() {
        // |1        | | |   | |
        // |1 2      | |2| = |4|
        // |  3 3    | |1|   |9|
        // |      7  | | |   | |
        // |  2   3 5| |1|   |9|
        let l = CsMatOwned::new_csc((5, 5),
                                    vec![0, 2, 5, 6, 8, 9],
                                    vec![0, 1, 1, 2, 4, 2, 3, 4, 4],
                                    vec![1, 1, 2, 3, 2, 3, 7, 3, 5]);
        let b = vec::CsVecOwned::new(5, vec![1, 2, 4], vec![4, 9, 9]);
        let mut xw = vec![1; 5]; // inital values should not matter
        let mut visited = vec![false; 5]; // inital values matter here
        let mut dstack = DStack::with_capacity(2 * 5);
        super::lsolve_csc_sparse_rhs(l.view(),
                                     b.view(),
                                     &mut dstack,
                                     &mut xw,
                                     &mut visited)
            .unwrap();

        let x: HashSet<_> = dstack.iter_right()
                                  .map(stack::extract_stack_val)
                                  .map(|&i| (i, xw[i]))
                                  .collect();

        let expected_output = vec::CsVecOwned::new(5,
                                                   vec![1, 2, 4],
                                                   vec![2, 1, 1]);
        let expected_output = expected_output.to_set();

        assert_eq!(x, expected_output);

        // |1            | |1|   |1|
        // |  2          | | | = | |
        // |1   3        | |2|   |7|
        // |      7      | |1|   |7|
        // |        5    | | |   | |
        // |    1     1  | |1|   |3|
        // |  3     2   2| | |   | |
        let l = CsMatOwned::new_csc((7, 7),
                                    vec![0, 2, 4, 6, 7, 9, 10, 11],
                                    vec![0, 2, 1, 6, 2, 5, 3, 4, 6,
                                         5, 6],
                                    vec![1, 1, 2, 3, 3, 1, 7, 5, 2,
                                         1, 2]);
        let b = vec::CsVecOwned::new(7,
                                     vec![0, 2, 3, 5],
                                     vec![1, 7, 7, 3]);
        let mut dstack = DStack::with_capacity(2 * 7);
        let mut xw = vec![1; 7]; // inital values should not matter
        let mut visited = vec![false; 7]; // inital values matter here

        super::lsolve_csc_sparse_rhs(l.view(),
                                     b.view(),
                                     &mut dstack,
                                     &mut xw,
                                     &mut visited)
            .unwrap();
        let x: HashSet<_> = dstack.iter_right()
                                  .map(stack::extract_stack_val)
                                  .map(|&i| (i, xw[i]))
                                  .collect();

        let expected_output = vec::CsVecOwned::new(7,
                                                   vec![0, 2, 3, 5],
                                                   vec![1, 2, 1, 1]
                                                  ).to_set();

        assert_eq!(x, expected_output);
    }
}
