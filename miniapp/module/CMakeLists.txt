# Vesting core module. The module lives entirely in the Rust crate under rust-lib/: the builder
# compiles it to a staticlib and links it into the plugin with the generated Qt glue.
cmake_minimum_required(VERSION 3.14)
project(VestingModulePlugin LANGUAGES CXX)

if(DEFINED ENV{LOGOS_MODULE_BUILDER_ROOT})
    include($ENV{LOGOS_MODULE_BUILDER_ROOT}/cmake/LogosModule.cmake)
elseif(EXISTS "${CMAKE_CURRENT_SOURCE_DIR}/cmake/LogosModule.cmake")
    include(cmake/LogosModule.cmake)
else()
    message(FATAL_ERROR "LogosModule.cmake not found")
endif()

file(READ "${CMAKE_CURRENT_SOURCE_DIR}/metadata.json" METADATA_JSON)
string(JSON MODULE_NAME GET ${METADATA_JSON} name)

logos_module(NAME ${MODULE_NAME})

# Native libraries the Rust staticlib needs beyond what the builder links for Rust std
# (CoreFoundation, Security): rustc's `--print native-static-libs` for vesting-module-core with
# `lez`. The plugin links with `-undefined dynamic_lookup`, so a missing one only shows when
# Basecamp loads it (PC/SC, for the LEZ wallet's Keycard support, did).
if(APPLE)
    target_link_libraries(${MODULE_NAME}_module_plugin PRIVATE
        "-framework PCSC" "-framework SystemConfiguration" "-framework Metal"
        "-framework CoreGraphics" objc iconv c++)
elseif(UNIX)
    # Linux: PC/SC (pcsclite, which the flake puts in the build inputs and the portable bundle
    # then carries) and the rest of rustc's list for linux-gnu beyond the builder's pthread/dl.
    # risc0's and rapidsnark's C++ resolve against libstdc++ from the C++ link itself. GNU ld
    # also lets a shared object keep undefined symbols, so --no-undefined makes a missing one
    # fail this link instead of Basecamp's dlopen.
    target_link_libraries(${MODULE_NAME}_module_plugin PRIVATE pcsclite m rt util)
    target_link_options(${MODULE_NAME}_module_plugin PRIVATE "-Wl,--no-undefined")
endif()
