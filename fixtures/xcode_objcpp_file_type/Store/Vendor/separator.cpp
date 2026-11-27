#include "separator.h"

#include <string>

static const std::string separator = "+";

const char *store_separator() { return separator.c_str(); }
