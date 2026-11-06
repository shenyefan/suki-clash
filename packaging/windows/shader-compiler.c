#define COBJMACROS
#include <windows.h>
#include <d3dcompiler.h>
#include <stdio.h>
#include <stdlib.h>

/* Build tool only: invoke Microsoft's D3DCompiler under Wine, never the GUI. */
int main(int argc, char **argv) {
    if (argc != 5) {
        fprintf(stderr, "usage: shader-compiler SOURCE ENTRY TARGET OUTPUT\n");
        return 1;
    }
    int length = MultiByteToWideChar(CP_UTF8, MB_ERR_INVALID_CHARS, argv[1], -1, NULL, 0);
    if (!length) return 1;
    WCHAR *source = malloc(length * sizeof(WCHAR));
    if (!source) return 1;
    MultiByteToWideChar(CP_UTF8, MB_ERR_INVALID_CHARS, argv[1], -1, source, length);
    HMODULE compiler = LoadLibraryW(L"d3dcompiler_47.dll");
    __typeof__(&D3DCompileFromFile) compile = compiler
        ? (__typeof__(&D3DCompileFromFile))GetProcAddress(compiler, "D3DCompileFromFile") : NULL;
    if (!compile) {
        fprintf(stderr, "Cannot load Microsoft D3DCompileFromFile: %lu\n", (unsigned long)GetLastError());
        free(source);
        return 1;
    }
    ID3DBlob *code = NULL, *errors = NULL;
    HRESULT result = compile(source, NULL, D3D_COMPILE_STANDARD_FILE_INCLUDE,
        argv[2], argv[3], D3DCOMPILE_OPTIMIZATION_LEVEL3, 0, &code, &errors);
    free(source);
    if (errors) {
        fputs((const char *)ID3D10Blob_GetBufferPointer(errors), stderr);
        ID3D10Blob_Release(errors);
    }
    if (FAILED(result)) {
        fprintf(stderr, "D3DCompileFromFile failed: 0x%08lx\n", (unsigned long)(DWORD)result);
        return 1;
    }
    FILE *output = fopen(argv[4], "wb");
    if (!output) {
        perror(argv[4]);
        ID3D10Blob_Release(code);
        return 1;
    }
    size_t size = ID3D10Blob_GetBufferSize(code);
    size_t written = fwrite(ID3D10Blob_GetBufferPointer(code), 1, size, output);
    ID3D10Blob_Release(code);
    int closed = fclose(output);
    if (written != size || closed != 0) {
        perror("write shader");
        return 1;
    }
    return 0;
}
