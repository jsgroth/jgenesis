#!/usr/bin/env python3
#  Based on https://www.allaboutcircuits.com/technical-articles/design-of-fir-filters-design-octave-matlab/

import sys

DEFAULT_CUTOFF = 15000.0
DEFAULT_STOPBAND_EDGE = 20000.0
DEFAULT_STOPBAND_ATTENUATION = 40.0

def main():
    if len(sys.argv) < 2:
        print( "Estimate the number of taps needed for a FIR low-pass filter")
        print( "  ARGS: Fs [f1] [f2] [dB]")
        print( "    Fs: source frequency (Hz)")
        print(f"    f1: cutoff frequency (Hz) (default={DEFAULT_CUTOFF})")
        print(f"    f2: stopband edge frequency (Hz) (default={DEFAULT_STOPBAND_EDGE})")
        print(f"    dB: stopband attenuation (dB) (default={DEFAULT_STOPBAND_ATTENUATION})")
        sys.exit(1)

    fs = float(sys.argv[1])
    f1 = float(sys.argv[2]) if len(sys.argv) >= 3 else DEFAULT_CUTOFF
    f2 = float(sys.argv[3]) if len(sys.argv) >= 4 else DEFAULT_STOPBAND_EDGE
    db = float(sys.argv[4]) if len(sys.argv) >= 5 else DEFAULT_STOPBAND_ATTENUATION

    delta_f = f2 - f1
    n = (db * fs) / (22 * delta_f)
    print(n)


if __name__ == "__main__":
    main()
