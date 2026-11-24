// Copyright 2006-2016 The OpenSSL Project Authors. All Rights Reserved.
//
// Licensed under the Apache License, Version 2.0 (the "License");
// you may not use this file except in compliance with the License.
// You may obtain a copy of the License at
//
//     https://www.apache.org/licenses/LICENSE-2.0
//
// Unless required by applicable law or agreed to in writing, software
// distributed under the License is distributed on an "AS IS" BASIS,
// WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
// See the License for the specific language governing permissions and
// limitations under the License.

#include <openssl/evp.h>
#include <openssl/dsa.h>
#include <openssl/err.h>
#include "internal.h"

const EVP_PKEY_ALG *EVP_pkey_dsa() {
  return nullptr;
}

int EVP_PKEY_CTX_set_dsa_paramgen_bits(EVP_PKEY_CTX *ctx, int nbits) {
  OPENSSL_PUT_ERROR(EVP, ERR_R_SHOULD_NOT_HAVE_BEEN_CALLED);
  return 0;
}

int EVP_PKEY_CTX_set_dsa_paramgen_q_bits(EVP_PKEY_CTX *ctx, int qbits) {
  OPENSSL_PUT_ERROR(EVP, ERR_R_SHOULD_NOT_HAVE_BEEN_CALLED);
  return 0;
}

int EVP_PKEY_set1_DSA(EVP_PKEY *pkey, DSA *key) {
  return 0;
}

int EVP_PKEY_assign_DSA(EVP_PKEY *pkey, DSA *key) {
  return 0;
}

DSA *EVP_PKEY_get0_DSA(const EVP_PKEY *pkey) {
  return nullptr;
}

DSA *EVP_PKEY_get1_DSA(const EVP_PKEY *pkey) {
  return nullptr;
}
