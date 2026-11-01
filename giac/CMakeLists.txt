# The Giac worker: a small program that reads Giac commands on stdin and
# answers each on one line of stdout. The app runs it as a separate process,
# so a long computation can be stopped by ending the process.
#
# Giac's sources come from GeoGebra's mirror (https://github.com/geogebra/giac),
# which ships static GMP and MPFR for each platform. Build on Windows with
# MSYS2's CLANG64 shell:
#
#   cmake -S giac -B giac-build -G Ninja -DCMAKE_BUILD_TYPE=Release \
#         -DGIAC_SRC=<checkout of geogebra/giac>
#   cmake --build giac-build
#
# Then set GIAC_WORKER to giac-build/giac-worker.exe when building the app,
# and the .exe carries the worker inside it.

cmake_minimum_required(VERSION 3.10)
project(giac_worker C CXX)

set(GIAC_SRC "" CACHE PATH "Checkout of github.com/geogebra/giac")
if(NOT EXISTS "${GIAC_SRC}/src/giac/cpp/gen.cc")
    message(FATAL_ERROR "Set GIAC_SRC to a checkout of github.com/geogebra/giac")
endif()

set(CPP ${GIAC_SRC}/src/giac/cpp)
set(GIAC_SOURCES
    alg_ext.cc cocoa.cc csturm.cc derive.cc desolve.cc ezgcd.cc
    freeglut_stroke_roman.c gauss.cc gausspol.cc gen.cc global.cc help.cc
    identificateur.cc ifactor.cc index.cc input_lexer.cc input_parser.cc
    intg.cc intgab.cc isom.cc lin.cc maple.cc mathml.cc misc.cc modfactor.cc
    modpoly.cc moyal.cc opengl.cc pari.cc permu.cc plot.cc plot3d.cc prog.cc
    quater.cc risch.cc rpn.cc series.cc solve.cc sparse.cc subst.cc
    sym2poly.cc symbolic.cc tex.cc threaded.cc ti89.cc tinymt32.cc
    TmpFGLM.cpp TmpLESystemSolver.cpp unary.cc usual.cc vecteur.cc)
list(TRANSFORM GIAC_SOURCES PREPEND ${CPP}/)

set(CMAKE_CXX_STANDARD 11)
set(CMAKE_CXX_FLAGS "${CMAKE_CXX_FLAGS} -fpermissive -w")
set(CMAKE_C_FLAGS "${CMAKE_C_FLAGS} -w")

add_executable(giac-worker ${GIAC_SOURCES} worker.cc)
target_include_directories(giac-worker PRIVATE ${GIAC_SRC}/src/giac/headers)
target_compile_definitions(giac-worker PRIVATE
    HAVE_NO_HOME_DIRECTORY GIAC_GGB IN_GIAC HAVE_LIB_PTHREAD
    GIAC_GENERIC_CONSTANTS TIMEOUT SIZEOF_VOID_P=8 HAVE_LIBMPFR VERSION="1.9")

set(PREBUILT ${GIAC_SRC}/src/jni/prebuilt)
if(WIN32)
    target_compile_definitions(giac-worker PRIVATE
        GIAC_MPQS __MINGW_H MINGW32 HAVE_NO_SYS_TIMES_H
        HAVE_NO_SYS_RESOURCE_WAIT_H HAVE_NO_PWD_H HAVE_NO_CWD NO_CLOCK
        usleep= YY_NO_UNISTD_H)
    set(LIBS ${PREBUILT}/windows/clang64)
    # One self-contained .exe: no libc++ or winpthread DLLs beside it.
    target_link_options(giac-worker PRIVATE -static -s)
else()
    target_compile_definitions(giac-worker PRIVATE
        HAVE_SYS_TIMES_H HAVE_UNISTD_H HAVE_SYS_TIME_H HAVE_SYSCONF)
    if(APPLE)
        set(LIBS ${PREBUILT}/maccatalyst/x86_64)
    else()
        set(LIBS ${PREBUILT}/linux/x86-64)
        target_link_options(giac-worker PRIVATE -static-libgcc -static-libstdc++)
    endif()
    target_link_libraries(giac-worker pthread)
endif()
# MPFR must come before GMP.
target_link_libraries(giac-worker ${LIBS}/libmpfr.a ${LIBS}/libgmp.a)
