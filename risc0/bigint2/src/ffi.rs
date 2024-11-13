// Copyright 2024 RISC Zero, Inc.
//
// Licensed under the Apache License, Version 2.0 (the "License");
// you may not use this file except in compliance with the License.
// You may obtain a copy of the License at
//
//     http://www.apache.org/licenses/LICENSE-2.0
//
// Unless required by applicable law or agreed to in writing, software
// distributed under the License is distributed on an "AS IS" BASIS,
// WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
// See the License for the specific language governing permissions and
// limitations under the License.

extern "C" {
    pub fn sys_bigint2_0(blob_ptr: *const u8);

    pub fn sys_bigint2_1(blob_ptr: *const u8, a1: *const u32);

    pub fn sys_bigint2_2(blob_ptr: *const u8, a1: *const u32, a2: *const u32);

    pub fn sys_bigint2_3(blob_ptr: *const u8, a1: *const u32, a2: *const u32, a3: *const u32);

    pub fn sys_bigint2_4(
        blob_ptr: *const u8,
        a1: *const u32,
        a2: *const u32,
        a3: *const u32,
        a4: *const u32,
    );

    pub fn sys_bigint2_5(
        blob_ptr: *const u8,
        a1: *const u32,
        a2: *const u32,
        a3: *const u32,
        a4: *const u32,
        a5: *const u32,
    );

    pub fn sys_bigint2_6(
        blob_ptr: *const u8,
        a1: *const u32,
        a2: *const u32,
        a3: *const u32,
        a4: *const u32,
        a5: *const u32,
        a6: *const u32,
    );

    pub fn sys_bigint2_7(
        blob_ptr: *const u8,
        a1: *const u32,
        a2: *const u32,
        a3: *const u32,
        a4: *const u32,
        a5: *const u32,
        a6: *const u32,
        a7: *const u32,
    );
}
