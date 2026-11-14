# eco-window: the Qt 6 program that runs the overlay's QML (docs/design.md §3).
# scripts/window-build builds it into target/window.
cmake_minimum_required(VERSION 3.24)
project(eco-window LANGUAGES CXX)

set(CMAKE_CXX_STANDARD 20)
set(CMAKE_CXX_STANDARD_REQUIRED ON)
set(CMAKE_AUTOMOC ON)
set(CMAKE_COMPILE_WARNING_AS_ERROR ON)

find_package(Qt6 6.5 REQUIRED COMPONENTS Gui Network Qml Quick Widgets)

qt_add_executable(eco-window main.cpp host.h line_socket.h text_file.h)
if(APPLE)
  # NSWindow's sharing type, which leaves a window out of screen capture, and
  # the pasteboard, which keeps a copied text.
  enable_language(OBJCXX)
  target_sources(eco-window PRIVATE bridge_macos.h bridge_macos.mm)
  target_link_libraries(eco-window PRIVATE "-framework AppKit")
  # The bundle, eco.app with eco's icon, as the app is named and drawn on
  # Linux, also carries the Qt it runs with once packaged.
  set(icon ${CMAKE_CURRENT_SOURCE_DIR}/../packaging/macos/eco.icns)
  target_sources(eco-window PRIVATE ${icon})
  set_source_files_properties(${icon} PROPERTIES MACOSX_PACKAGE_LOCATION Resources)
  set_target_properties(eco-window PROPERTIES MACOSX_BUNDLE ON OUTPUT_NAME eco
    MACOSX_BUNDLE_INFO_PLIST ${CMAKE_CURRENT_SOURCE_DIR}/Info.plist)
endif()
if(ANDROID)
  # An Android app is a library the activity loads, and the APK carries the
  # overlay's files as resources, since no QML file sits beside it. The daemon,
  # built apart (scripts/android-build, ECO_DAEMON), ships as libeco.so, which
  # Android extracts into the app's native library directory.
  file(GLOB_RECURSE overlay_files ${CMAKE_CURRENT_SOURCE_DIR}/../overlay/*.qml
    ${CMAKE_CURRENT_SOURCE_DIR}/../overlay/*.js ${CMAKE_CURRENT_SOURCE_DIR}/../overlay/*.json
    ${CMAKE_CURRENT_SOURCE_DIR}/../overlay/*.png ${CMAKE_CURRENT_SOURCE_DIR}/../overlay/qmldir)
  qt_add_resources(eco-window overlay PREFIX /overlay
    BASE ${CMAKE_CURRENT_SOURCE_DIR}/../overlay FILES ${overlay_files})
  set_target_properties(eco-window PROPERTIES
    QT_ANDROID_PACKAGE_SOURCE_DIR ${CMAKE_CURRENT_SOURCE_DIR}/android
    QT_ANDROID_MIN_SDK_VERSION 26
    QT_ANDROID_TARGET_SDK_VERSION 36
    QT_ANDROID_LEGACY_PACKAGING ON
    QT_ANDROID_EXTRA_LIBS ${ECO_DAEMON})
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
