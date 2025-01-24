"""
Generate a Butterworth IIR high-pass filter, and plot the frequency response.
"""

import argparse
from math import pi

import matplotlib.pyplot as plt
import numpy as np
from scipy.signal import freqz, iirfilter


def main():
    args = parse_args()

    b, a = iirfilter(args.n, args.fc / (args.fs / 2), ftype="butter", btype="highpass")
    print(list(b))
    print(list(a))

    w, h = freqz(b, a, worN=1 << 20)

    figure = plt.figure()
    axes = figure.add_subplot(
        title="Filter Frequency Response",
        xlabel="Frequency (Hz)",
        ylabel="Gain (dB)",
        xlim=(0, args.fc * 4),
        ylim=(-80, 20),
    )
    axes.grid(visible=True)

    axes.plot(w * (args.fs / 2) / pi, 20 * np.log10(abs(h)))

    plt.show(block=True)


def parse_args():
    arg_parser = argparse.ArgumentParser(
        description="Generate a Butterworth IIR high-pass filter"
    )
    arg_parser.add_argument(
        "-fs", type=float, required=True, help="Source frequency (Hz)"
    )
    arg_parser.add_argument("-fc", type=float, required=True, help="Cutoff frequency (Hz)")
    arg_parser.add_argument("-n", type=int, required=True, help="Filter order")
    return arg_parser.parse_args()


if __name__ == "__main__":
    main()
