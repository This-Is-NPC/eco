# eco-window: the Qt 6 program that runs the overlay's QML (docs/design.md §3).
# scripts/window-build builds it into target/window.
cmake_minimum_required(VERSION 3.24)
project(eco-window LANGUAGES CXX)

set(CMAKE_CXX_STANDARD 20)
set(CMAKE_CXX_STANDARD_REQUIRED ON)
set(CMAKE_AUTOMOC ON)
set(CMAKE_COMPILE_WARNING_AS_ERROR ON)

find_package(Qt6 6.5 REQUIRED COMPONENTS Gui Network Qml Quick Widgets)

add_executable(eco-window main.cpp host.h line_socket.h text_file.h)
if(APPLE)
  # NSWindow's sharing type, which leaves a window out of screen capture, and
  # the pasteboard, which keeps a copied text.
  enable_language(OBJCXX)
  target_sources(eco-window PRIVATE bridge_macos.h bridge_macos.mm)
  target_link_libraries(eco-window PRIVATE "-framework AppKit")
  # The microphone permission: Qt's plugin asks the system, which needs the
  # usage text in an Info.plist, embedded since eco-window is no bundle.
  target_link_libraries(eco-window PRIVATE Qt6::QDarwinMicrophonePermissionPlugin)
  target_link_options(eco-window PRIVATE "LINKER:-sectcreate,__TEXT,__info_plist,${CMAKE_CURRENT_SOURCE_DIR}/Info.plist")
endif()
if(WIN32)
  target_sources(eco-window PRIVATE eco-window.rc)
  # OleFlushClipboard, which leaves a copied text on the clipboard.
  target_link_libraries(eco-window PRIVATE ole32)
endif()
if(MSVC)
  target_compile_options(eco-window PRIVATE /W4 /utf-8)
else()
  target_compile_options(eco-window PRIVATE -Wall -Wextra -Wpedantic)
endif()
target_compile_definitions(eco-window PRIVATE QT_NO_CAST_FROM_ASCII QT_NO_KEYWORDS)
target_link_libraries(eco-window PRIVATE Qt6::Gui Qt6::Network Qt6::Qml Qt6::Quick Qt6::Widgets)
