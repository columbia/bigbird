"""The sole read-side metric: analytic RMSRE_tau.

Determinism note: every figure number is computed ANALYTICALLY from the Laplace
noise scale ``b`` (per-bin variance ``2 b^2``) plus the truncation bias
``(filtered - unfiltered)^2``. The sampled ``noisy_aggregation`` written by the
Rust runtime is never read here, which is why the figures are bit-deterministic
across runs and thread counts.
"""

import numpy as np


def rmsre_tau(filtered, unfiltered, noise_scale, tau):
    r"""RMSRE_tau = sqrt( mean_i (2 b^2 + (f_i - u_i)^2) / max(u_i, tau)^2 ).

    ``2 b^2`` is the DP Laplace-noise variance per histogram bin (``b`` =
    ``noise_scale``); ``(f_i - u_i)^2`` is the truncation bias from
    budget-dropped epochs. Pass ``noise_scale=0`` to get the bias-only
    "relative_bias". Histograms have m = 5 bins, so numpy sums sequentially and
    this stays bit-identical to the original per-coordinate loop.
    """
    numerator = 2 * noise_scale**2 + np.square(filtered - unfiltered)
    denominator = np.square(np.maximum(unfiltered, tau))
    return float(np.sqrt(np.mean(numerator / denominator)))
