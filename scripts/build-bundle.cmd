@echo off
rem Build Windows x64 CPU binaries and a complete native bundle. No services or models.
setlocal
cd /d "%~dp0.."
if not defined VCToolsInstallDir (
 for /f "usebackq tokens=*" %%i in (`"%ProgramFiles(x86)%\Microsoft Visual Studio\Installer\vswhere.exe" -latest -products * -requires Microsoft.VisualStudio.Component.VC.Tools.x86.x64 -property installationPath`) do set "VSROOT=%%i"
)
if not defined VCToolsInstallDir call "%VSROOT%\Common7\Tools\VsDevCmd.bat" -arch=x64 -host_arch=x64
if errorlevel 1 exit /b 1
set CARGO_BUILD_JOBS=2
set CMAKE_BUILD_PARALLEL_LEVEL=2
set CMAKE_GENERATOR=Ninja
cargo build --locked --release --no-default-features --features qwentts,playback --bins
if errorlevel 1 exit /b 1
cargo fetch --locked --target x86_64-pc-windows-msvc
if errorlevel 1 exit /b 1
if not defined SPQX_QWENTTS_SOURCE set "SPQX_QWENTTS_SOURCE=%CD%\target\qwentts-source"
if not exist "%SPQX_QWENTTS_SOURCE%\.git" (
 git clone --no-checkout https://github.com/ServeurpersoCom/qwentts.cpp.git "%SPQX_QWENTTS_SOURCE%"
 if errorlevel 1 exit /b 1
 git -C "%SPQX_QWENTTS_SOURCE%" checkout --detach 5916bf7c79dbb4e66fc059cdc507a79a23a28bb4
 if errorlevel 1 exit /b 1
 git -C "%SPQX_QWENTTS_SOURCE%" submodule update --init --recursive
 if errorlevel 1 exit /b 1
)
for /f %%i in ('git -C "%SPQX_QWENTTS_SOURCE%" rev-parse HEAD') do if not "%%i"=="5916bf7c79dbb4e66fc059cdc507a79a23a28bb4" exit /b 2
for /f %%i in ('git -C "%SPQX_QWENTTS_SOURCE%\ggml" rev-parse HEAD') do if not "%%i"=="cc5df49a6b878db2b68b9a7b45976e279594627d" exit /b 2
git -C "%SPQX_QWENTTS_SOURCE%" diff --quiet
if errorlevel 1 exit /b 2
git -C "%SPQX_QWENTTS_SOURCE%" diff --cached --quiet
if errorlevel 1 exit /b 2
git -C "%SPQX_QWENTTS_SOURCE%\ggml" diff --quiet
if errorlevel 1 exit /b 2
git -C "%SPQX_QWENTTS_SOURCE%\ggml" diff --cached --quiet
if errorlevel 1 exit /b 2
if exist target\bundle-runtime-cpu\lib exit /b 2
cmake -S "%SPQX_QWENTTS_SOURCE%" -B target\bundle-runtime-cpu\build -G Ninja -DQWEN_SHARED=ON -DBUILD_SHARED_LIBS=ON -DGGML_BACKEND_DL=OFF -DGGML_NATIVE=OFF -DGGML_CUDA=OFF -DGGML_VULKAN=OFF -DGGML_METAL=OFF -DGGML_BLAS=OFF -DGGML_OPENMP=OFF -DCMAKE_BUILD_TYPE=Release
if errorlevel 1 exit /b 1
cmake --build target\bundle-runtime-cpu\build --target qwen --parallel 2
if errorlevel 1 exit /b 1
mkdir target\bundle-runtime-cpu\lib
copy /Y target\bundle-runtime-cpu\build\*.dll target\bundle-runtime-cpu\lib\ >nul
if not defined VCToolsRedistDir exit /b 2
set "EXTRAS="
for %%i in ("%VCToolsRedistDir%x64\Microsoft.VC143.CRT\*.dll") do call set EXTRAS=%%EXTRAS%% --extra-library "%%~fi"
uv run --no-project scripts\vendor_notices.py target\MSVC-REDISTRIBUTION.txt
if errorlevel 1 exit /b 1
uv run --no-project scripts\package.py --target x86_64-pc-windows-msvc --backend cpu --features qwentts,playback --binaries target\release --runtime target\bundle-runtime-cpu\lib --native-source "%SPQX_QWENTTS_SOURCE%" --output dist\release --extra-license target\MSVC-REDISTRIBUTION.txt --extra-license target\MSVC-RUNTIME-LICENSE.txt --extra-license target\MSVC-RUNTIME-LICENSE.docx %EXTRAS% --requirement "Windows 10/11 x64 with AVX2; no GPU, Python or libtorch required"
exit /b %ERRORLEVEL%
