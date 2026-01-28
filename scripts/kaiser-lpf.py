# /// script
# dependencies = [
#   "matplotlib",
#   "scipy",
# ]
# ///

"""
Generate an FIR low-pass filter designed for an oversampled signal using the windowing method with a Kaiser window, and
plot the frequency response.
"""

import argparse
from math import pi

import matplotlib.pyplot as plt
import numpy as np
from scipy.signal import firwin, freqz, kaiser_beta


def main():
    args = parse_args()

    beta = kaiser_beta(args.sba)
    taps = args.n * (2 * args.nz + 1)
    coefficients = firwin(
        taps, (args.fc / (args.cutoff_denominator_freq / 2)) / args.n, window=("kaiser", beta)
    )

    print(f"{taps} taps")

    with open(args.o, "w") as f:
        coeffs_to_write = coefficients
        if not args.write_full:
            coeffs_to_write = coefficients[int(len(coefficients) / 2) :]
        for coefficient in coeffs_to_write:
            f.write(f"{coefficient},\n")

    w, h = freqz(coefficients, worN=1 << 20)

    figure = plt.figure()
    axes = figure.add_subplot(
        title="Filter Frequency Response",
        xlabel="Frequency (Hz)",
        ylabel="Gain (dB)",
        xlim=(0, min(args.fs, 4 * args.fc)),
        ylim=(-(args.sba + 20), 20),
    )
    axes.grid(visible=True)

    axes.plot(w * args.n * (args.fs / 2) / pi, 20 * np.log10(abs(h)))
    axes.plot(2 * [args.fs / 2], [-300, 300])
    axes.plot([0, args.fs], 2 * [-3])

    plt.show(block=True)


def parse_args():
    arg_parser = argparse.ArgumentParser(
        description="Generate a FIR low-pass filter using a Kaiser window"
    )
    arg_parser.add_argument(
        "-sba",
        default=80,
        type=float,
        required=False,
        help="Stopband attenuation (dB) (default=80)",
    )
    arg_parser.add_argument(
        "-n",
        default=512,
        type=int,
        required=False,
        help="Samples per zero crossing (default=512)",
    )
    arg_parser.add_argument(
        "-nz", type=int, required=True, help="Number of zero crossings"
    )
    arg_parser.add_argument(
        "-fs",
        default=48000,
        type=float,
        required=False,
        help="Source frequency for plotting (Hz) (default=48000)",
    )
    arg_parser.add_argument(
        "-fc",
        default=20000,
        type=float,
        required=False,
        help="Cutoff frequency (Hz) (default=20000)",
    )
    arg_parser.add_argument(
        "--cutoff-denominator-freq",
        default=48000,
        type=float,
        required=False,
        help="Denominator when computing cutoff value (Hz) (default=48000)"
    )
    arg_parser.add_argument(
        "-o",
        default="kaiser-fir.txt",
        type=str,
        required=False,
        help="Output file (default=kaiser-fir.txt)",
    )
    arg_parser.add_argument(
        "--write-full",
        action="store_true",
        required=False,
        help="Write out the entire filter instead of only the right half",
    )
    return arg_parser.parse_args()


if __name__ == "__main__":
    main()
