int store_checksum(const char *text) {
    int sum = 0;
    while (*text) {
        sum += *text++;
    }
    return sum;
}
