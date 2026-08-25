@echo off
rem Wrapper for building C++ code targeting wasm32-unknown-unknown.
rem See tools/wasm32-clang++.js for details.
node "%~dp0wasm32-clang++.js" %*
