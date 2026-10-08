// Original C ABI target for the owned IL fixture. No closures or code emission.
typedef struct { double x; long long y; } ProbePair;
ProbePair wire_probe_pair(ProbePair value, double increment) {
    value.x += increment;
    value.y += 2;
    return value;
}
double wire_probe_callback(double (*callback)(double, int), double value, int factor) {
    return callback(value, factor);
}
