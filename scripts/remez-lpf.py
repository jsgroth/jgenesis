"""
Generate a FIR low-pass filter using the Remez exchange algorithm, and plot the frequency response.
"""

import argparse
from math import pi

import matplotlib.pyplot as plt
import numpy as np
from scipy.signal import freqz, remez


def main():
    args = parse_args()

    weight_pb = 1 - 10 ** (-args.pbr / 20)
    weight_sb = 10 ** (-args.sba / 20)
    coefficients = remez(
        args.n,
        [0, args.fp, args.fe, args.fs / 2],
        [1, 0],
        weight=[weight_pb, weight_sb],
        fs=args.fs,
    )
    print(list(coefficients))

    w, h = freqz(coefficients, worN=1 << 20)

    figure = plt.figure()
    axes = figure.add_subplot(
        title="Filter Frequency Response",
        xlabel="Frequency (Hz)",
        ylabel="Gain (dB)",
        xlim=(0, 40000),
        ylim=(-80, 20),
    )
    axes.grid(visible=True)

    axes.plot(w * (args.fs / 2) / pi, 20 * np.log10(abs(h)))
    axes.plot(2 * [24000], [-100, 100])
    axes.plot([0, args.fs], 2 * [-3])

    plt.show(block=True)


def parse_args():
    arg_parser = argparse.ArgumentParser(
        description="Generate a FIR low-pass filter using the Remez exchange algorithm"
    )
    arg_parser.add_argument(
        "-fs", type=float, required=True, help="Source frequency (Hz)"
    )
    arg_parser.add_argument("-n", type=int, required=True, help="Number of taps")
    arg_parser.add_argument(
        "-fp",
        default=16000,
        type=float,
        required=False,
        help="Transition band start frequency (Hz) (default=16000)",
    )
    arg_parser.add_argument(
        "-fe",
        default=24000,
        type=float,
        required=False,
        help="Stopband edge frequency (Hz) (default=24000)",
    )
    arg_parser.add_argument(
        "-pbr",
        default=0.1,
        type=float,
        required=False,
        help="Desired passband ripple (dB) (default=0.1)",
    )
    arg_parser.add_argument(
        "-sba",
        default=60,
        type=float,
        required=False,
        help="Desired stopband attenuation (dB) (default=60)",
    )
    return arg_parser.parse_args()


if __name__ == "__main__":
    main()
