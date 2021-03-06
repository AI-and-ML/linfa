use std::cmp::Ordering;

use super::permutable_kernel::{Kernel, PermutableKernel, PermutableKernelOneClass};
use super::solver_smo::SolverState;
use super::SolverParams;
use super::{Float, SvmResult};

/// Support Vector Classification with C-penalizing parameter
///
/// This methods solves a binary SVC problem with a penalizing parameter C between (0, inf). The
/// dual problem has the form
/// ```ignore
/// min_a 1/2*a^tQ a - e^T a s.t. y^t = 0, 0 <= a_i <= C_i
/// ```
/// with `Q_ij = y_i y_j K(x_i, x_j)` the kernel matrix.
///
/// # Parameters
///
/// * `params` - Solver parameters (threshold etc.)
/// * `kernel` - the kernel matrix `Q`
/// * `targets` - the ground truth targets `y_i`
/// * `cpos` - C for positive targets
/// * `cneg` - C for negative targets
pub fn fit_c<'a, A: Float>(
    params: &'a SolverParams<A>,
    kernel: &'a Kernel<A>,
    targets: &'a [bool],
    cpos: A,
    cneg: A,
) -> SvmResult<'a, A> {
    let bounds = targets
        .iter()
        .map(|x| if *x { cpos } else { cneg })
        .collect::<Vec<_>>();

    let kernel = PermutableKernel::new(kernel, targets.to_vec());

    let solver = SolverState::new(
        vec![A::zero(); targets.len()],
        vec![-A::one(); targets.len()],
        targets.to_vec(),
        kernel,
        bounds,
        params,
        false,
    );

    let mut res = solver.solve();

    res.alpha = res
        .alpha
        .into_iter()
        .zip(targets.iter())
        .map(|(a, b)| if *b { a } else { -a })
        .collect();

    res
}

/// Support Vector Classification with Nu-penalizing term
///
/// This methods solves a binary SVC problem with a penalizing parameter nu between (0, 1). The
/// dual problem has the form
/// ```ignore
/// min_a 1/2*a^tQ a s.t. y^t a = 0, 0 <= a_i <= 1/l, e^t a > nu
/// ```
/// with `Q_ij = y_i y_j K(x_i, x_j)` the kernel matrix.
///
/// # Parameters
///
/// * `params` - Solver parameters (threshold etc.)
/// * `kernel` - the kernel matrix `Q`
/// * `targets` - the ground truth targets `y_i`
/// * `nu` - Nu penalizing term
pub fn fit_nu<'a, A: Float>(
    params: &'a SolverParams<A>,
    kernel: &'a Kernel<A>,
    targets: &'a [bool],
    nu: A,
) -> SvmResult<'a, A> {
    let mut sum_pos = nu * A::from(targets.len()).unwrap() / A::from(2.0).unwrap();
    let mut sum_neg = nu * A::from(targets.len()).unwrap() / A::from(2.0).unwrap();
    let init_alpha = targets
        .iter()
        .map(|x| {
            if *x {
                let val = A::min(A::one(), sum_pos);
                sum_pos -= val;
                val
            } else {
                let val = A::min(A::one(), sum_neg);
                sum_neg -= val;
                val
            }
        })
        .collect::<Vec<_>>();

    let kernel = PermutableKernel::new(kernel, targets.to_vec());

    let solver = SolverState::new(
        init_alpha,
        vec![A::zero(); targets.len()],
        targets.to_vec(),
        kernel,
        vec![A::one(); targets.len()],
        params,
        true,
    );

    let mut res = solver.solve();

    let r = res.r.unwrap();

    res.alpha = res
        .alpha
        .into_iter()
        .zip(targets.iter())
        .map(|(a, b)| if *b { a } else { -a })
        .map(|x| x / r)
        .collect();
    res.rho /= r;
    res.obj /= r * r;

    res
}

/// Support Vector Classification for one-class problems
///
/// This methods solves a binary SVC, when there are no targets available. This can, for example be
/// useful, when outliers should be rejected.
///
/// # Parameters
///
/// * `params` - Solver parameters (threshold etc.)
/// * `kernel` - the kernel matrix `Q`
/// * `nu` - Nu penalizing term
pub fn fit_one_class<'a, A: Float + num_traits::ToPrimitive>(
    params: &'a SolverParams<A>,
    kernel: &'a Kernel<A>,
    nu: A,
) -> SvmResult<'a, A> {
    let size = kernel.size();
    let n = (nu * A::from(size).unwrap()).to_usize().unwrap();

    let init_alpha = (0..size)
        .map(|x| match x.cmp(&n) {
            Ordering::Less => A::one(),
            Ordering::Greater => A::zero(),
            Ordering::Equal => nu * A::from(size).unwrap() - A::from(x).unwrap(),
        })
        .collect::<Vec<_>>();

    let kernel = PermutableKernelOneClass::new(kernel);

    let solver = SolverState::new(
        init_alpha,
        vec![A::zero(); size],
        vec![true; size],
        kernel,
        vec![A::one(); size],
        params,
        false,
    );

    solver.solve()
}

#[cfg(test)]
mod tests {
    use super::{fit_c, fit_nu, fit_one_class, SolverParams};
    use linfa::metrics::IntoConfusionMatrix;
    use linfa_kernel::Kernel;
    use ndarray::{Array, Array2, Axis};
    use ndarray_rand::rand_distr::Uniform;
    use ndarray_rand::RandomExt;

    pub fn generate_convoluted_rings(n_points: usize) -> Array2<f64> {
        let mut out = Array::random((n_points * 2, 2), Uniform::new(0f64, 1.));
        for (i, mut elm) in out.outer_iter_mut().enumerate() {
            // generate convoluted rings with 1/10th noise
            let phi = 6.28 * elm[1];
            let eps = elm[0] / 10.0;

            if i < n_points {
                elm[0] = 1.0 * phi.cos() + eps;
                elm[1] = 1.0 * phi.sin() + eps;
            } else {
                elm[0] = 5.0 * phi.cos() + eps;
                elm[1] = 5.0 * phi.sin() + eps;
            }
        }

        out
    }

    #[test]
    fn test_linear_classification() {
        let entries = ndarray::stack(
            Axis(0),
            &[
                Array::random((10, 2), Uniform::new(-1., -0.5)).view(),
                Array::random((10, 2), Uniform::new(0.5, 1.)).view(),
            ],
        )
        .unwrap();
        let targets = (0..20).map(|x| x < 10).collect::<Vec<_>>();

        let kernel = Kernel::linear(&entries);

        let params = SolverParams {
            eps: 1e-3,
            shrinking: false,
        };

        // test C Support Vector Classification
        let svc = fit_c(&params, &kernel, &targets, 1.0, 1.0);

        let pred = entries
            .outer_iter()
            .map(|x| svc.predict(x))
            .map(|x| x > 0.0)
            .collect::<Vec<_>>();

        let cm = pred.into_confusion_matrix(&targets);
        assert_eq!(cm.accuracy(), 1.0);

        // test nu Support Vector Classification
        let svc = fit_nu(&params, &kernel, &targets, 0.01);
        println!("{}", svc);

        let pred = entries
            .outer_iter()
            .map(|x| svc.predict(x))
            .map(|x| x > 0.0)
            .collect::<Vec<_>>();

        let cm = pred.into_confusion_matrix(&targets);
        assert_eq!(cm.accuracy(), 1.0);
    }

    #[test]
    fn test_polynomial_classification() {
        // construct parabolica and classify middle area as positive and borders as negative
        let dataset = Array::random((40, 1), Uniform::new(-2f64, 2.));
        let targets = dataset.map_axis(Axis(1), |x| x[0] * x[0] < 0.5).to_vec();

        // choose a polynomial kernel, which corresponds to the parabolical data
        let kernel = Kernel::polynomial(&dataset, 0.0, 2.0);

        let params = SolverParams {
            eps: 1e-3,
            shrinking: false,
        };

        // test C Support Vector Classification
        let svc = fit_c(&params, &kernel, &targets, 1.0, 1.0);
        println!("C {}", svc);

        let pred = dataset
            .outer_iter()
            .map(|x| svc.predict(x))
            .map(|x| x > 0.0)
            .collect::<Vec<_>>();

        let cm = pred.into_confusion_matrix(&targets);
        assert!(cm.accuracy() > 0.9);

        // test nu Support Vector Classification
        let svc = fit_nu(&params, &kernel, &targets, 0.01);
        println!("Nu {}", svc);

        let pred = dataset
            .outer_iter()
            .map(|x| svc.predict(x))
            .map(|x| x > 0.0)
            .collect::<Vec<_>>();

        let cm = pred.into_confusion_matrix(&targets);
        assert!(cm.accuracy() > 0.9);
    }

    #[test]
    fn test_convoluted_rings_classification() {
        let dataset = generate_convoluted_rings(10);
        let targets = (0..20).map(|x| x < 10).collect::<Vec<_>>();
        let kernel = Kernel::gaussian(&dataset, 50.0);

        let params = SolverParams {
            eps: 1e-3,
            shrinking: false,
        };

        let svc = fit_c(&params, &kernel, &targets, 1.0, 1.0);

        let pred = dataset
            .outer_iter()
            .map(|x| svc.predict(x))
            .map(|x| x > 0.0)
            .collect::<Vec<_>>();

        let cm = pred.into_confusion_matrix(&targets);
        assert_eq!(cm.accuracy(), 1.0);

        let svc = fit_nu(&params, &kernel, &targets, 0.01);

        let pred = dataset
            .outer_iter()
            .map(|x| svc.predict(x))
            .map(|x| x > 0.0)
            .collect::<Vec<_>>();

        let cm = pred.into_confusion_matrix(&targets);
        assert_eq!(cm.accuracy(), 1.0);
    }

    #[test]
    fn test_reject_classification() {
        // generate two clusters with 100 samples each
        let entries = Array::random((100, 2), Uniform::new(-4., 4.));
        let kernel = Kernel::gaussian(&entries, 100.);

        let params = SolverParams {
            eps: 1e-3,
            shrinking: false,
        };

        let svc = fit_one_class(&params, &kernel, 0.1);
        println!("{}", svc);

        // now test that points outside the circle are rejected
        let validation = Array::random((100, 2), Uniform::new(-10., 10f32));
        let pred = validation
            .outer_iter()
            .map(|x| svc.predict(x) > 0.0)
            .collect::<Vec<_>>();

        // count the number of correctly rejected samples
        let mut rejected = 0;
        let mut total = 0;
        for (pred, pos) in pred.iter().zip(validation.outer_iter()) {
            let distance = (pos[0] * pos[0] + pos[1] * pos[1]).sqrt();
            if distance >= 5.0 {
                if !pred {
                    rejected += 1;
                }
                total += 1;
            }
        }

        // at least 95% should be correctly rejected
        assert!((rejected as f32) / (total as f32) > 0.95);
    }
}
