get_filename_component(_tui_test_prefix "${CMAKE_CURRENT_LIST_DIR}/../../.." ABSOLUTE)

if(NOT TARGET tui-test::ffi)
  add_library(tui-test::ffi SHARED IMPORTED)
  set_target_properties(tui-test::ffi PROPERTIES
    INTERFACE_INCLUDE_DIRECTORIES "${_tui_test_prefix}/include")
  if(WIN32)
    set_target_properties(tui-test::ffi PROPERTIES
      IMPORTED_LOCATION "${_tui_test_prefix}/bin/tui_test_ffi.dll"
      IMPORTED_IMPLIB "${_tui_test_prefix}/lib/tui_test_ffi.lib")
  elseif(APPLE)
    set_target_properties(tui-test::ffi PROPERTIES
      IMPORTED_LOCATION "${_tui_test_prefix}/lib/libtui_test_ffi.dylib")
  else()
    set_target_properties(tui-test::ffi PROPERTIES
      IMPORTED_NO_SONAME TRUE
      IMPORTED_LOCATION "${_tui_test_prefix}/lib/libtui_test_ffi.so")
  endif()
endif()

unset(_tui_test_prefix)
