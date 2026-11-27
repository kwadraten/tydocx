#ifndef TYDOCX_H
#define TYDOCX_H

#include <stddef.h>

#ifdef __cplusplus
extern "C" {
#endif

#define TYDOCX_SUCCESS 0
#define TYDOCX_INVALID_ARGUMENT 1
#define TYDOCX_INPUT_ERROR 2
#define TYDOCX_COMPILATION_ERROR 3
#define TYDOCX_OUTPUT_ERROR 4
#define TYDOCX_INTERNAL_ERROR 5

/*
 * Convert one UTF-8 Typst path to one UTF-8 DOCX path.
 * compat is "c1", "c2", or "c3"; NULL selects the default "c2".
 * Returns TYDOCX_SUCCESS or one of the error constants above.
 */
int tydocx_convert_file(
    const char *input_path,
    const char *output_path,
    const char *compat);

/*
 * Return the required size (including NUL) of this thread's last error and,
 * when buffer is non-NULL and buffer_len is nonzero, copy a NUL-terminated
 * UTF-8 message into it. Truncated messages are still NUL-terminated.
 * No native allocation is returned, so no error-free function is necessary.
 * Zero means error retrieval itself failed unexpectedly.
 */
size_t tydocx_last_error_message(char *buffer, size_t buffer_len);

#ifdef __cplusplus
}
#endif

#endif
