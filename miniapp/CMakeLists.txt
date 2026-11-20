cmake_minimum_required(VERSION 3.14)
project(VestingUiPlugin LANGUAGES CXX)

if(DEFINED ENV{LOGOS_MODULE_BUILDER_ROOT})
    include($ENV{LOGOS_MODULE_BUILDER_ROOT}/cmake/LogosModule.cmake)
else()
    message(FATAL_ERROR "LogosModule.cmake not found. Set LOGOS_MODULE_BUILDER_ROOT.")
endif()

file(READ "${CMAKE_CURRENT_SOURCE_DIR}/metadata.json" METADATA_JSON)
string(JSON MODULE_NAME GET ${METADATA_JSON} name)

# The backend is a local mock: JSON in memory, no network. QJson lives in Qt6::Core.
find_package(Qt6 REQUIRED COMPONENTS Core)

logos_module(
    NAME ${MODULE_NAME}
    REP_FILE src/vesting_ui.rep
    SOURCES
        src/vesting_ui_backend.h
        src/vesting_ui_backend.cpp
    INCLUDE_DIRS
        src
    LINK_LIBRARIES
        Qt6::Core
)
