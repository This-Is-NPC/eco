# eco-window: the Qt 6 program that runs the overlay's QML (docs/design.md §3).
# scripts/window-build builds it into target/window.
cmake_minimum_required(VERSION 3.24)
project(eco-window LANGUAGES CXX)

set(CMAKE_CXX_STANDARD 20)
set(CMAKE_CXX_STANDARD_REQUIRED ON)
set(CMAKE_AUTOMOC ON)
set(CMAKE_COMPILE_WARNING_AS_ERROR ON)

find_package(Qt6 6.5 REQUIRED COMPONENTS Gui Network Qml Quick)

add_executable(eco-window main.cpp host.h line_socket.h text_file.h)
target_compile_options(eco-window PRIVATE -Wall -Wextra -Wpedantic)
target_compile_definitions(eco-window PRIVATE QT_NO_CAST_FROM_ASCII QT_NO_KEYWORDS)
target_link_libraries(eco-window PRIVATE Qt6::Gui Qt6::Network Qt6::Qml Qt6::Quick)
