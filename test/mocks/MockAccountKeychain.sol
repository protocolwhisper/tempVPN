// SPDX-License-Identifier: MIT
pragma solidity ^0.8.24;

import {IAccountKeychain} from "../../src/interfaces/IAccountKeychain.sol";

contract MockAccountKeychain is IAccountKeychain {
    struct LimitState {
        uint256 remaining;
        uint64 periodEnd;
    }

    struct StoredSelectorRule {
        bytes4 selector;
        address[] recipients;
    }

    struct StoredCallScope {
        address target;
        StoredSelectorRule[] selectorRules;
    }

    struct ScopeState {
        bool isScoped;
        StoredCallScope[] scopes;
    }

    mapping(address account => mapping(address keyId => KeyInfo key)) internal keys;
    mapping(address account => mapping(address keyId => mapping(address token => LimitState limit))) internal limits;
    mapping(address account => mapping(address keyId => ScopeState scope)) internal scopes;
    mapping(address account => mapping(bytes32 witness => bool burned)) internal burnedWitnesses;

    modifier onlyRootTransaction() {
        if (msg.sender != tx.origin) revert UnauthorizedCaller();
        _;
    }

    function authorizeKey(
        address keyId,
        SignatureType signatureType,
        uint64 expiry,
        bool enforceLimits,
        LegacyTokenLimit[] calldata legacyLimits
    ) external onlyRootTransaction {
        TokenLimit[] memory converted = new TokenLimit[](legacyLimits.length);
        for (uint256 i; i < legacyLimits.length; i++) {
            converted[i] = TokenLimit({token: legacyLimits[i].token, amount: legacyLimits[i].amount, period: 0});
        }

        CallScope[] memory emptyScopes;
        KeyRestrictions memory config = KeyRestrictions({
            expiry: expiry,
            enforceLimits: enforceLimits,
            limits: converted,
            allowAnyCalls: true,
            allowedCalls: emptyScopes
        });
        _authorizeKey(msg.sender, keyId, signatureType, config);
    }

    function authorizeKey(address keyId, SignatureType signatureType, KeyRestrictions calldata config)
        external
        onlyRootTransaction
    {
        _authorizeKey(msg.sender, keyId, signatureType, config);
    }

    function authorizeKey(
        address keyId,
        SignatureType signatureType,
        KeyRestrictions calldata config,
        bytes32 witness
    ) external onlyRootTransaction {
        if (burnedWitnesses[msg.sender][witness]) revert KeyAuthorizationWitnessAlreadyBurned();
        _authorizeKey(msg.sender, keyId, signatureType, config);
        emit KeyAuthorizationWitness(msg.sender, witness);
    }

    function authorizeAdminKey(address keyId, SignatureType signatureType, bytes32 witness)
        external
        onlyRootTransaction
    {
        if (keyId == address(0) || keyId == msg.sender) revert InvalidKeyId();
        if (burnedWitnesses[msg.sender][witness]) revert KeyAuthorizationWitnessAlreadyBurned();
        TokenLimit[] memory emptyLimits;
        CallScope[] memory emptyScopes;
        KeyRestrictions memory config = KeyRestrictions({
            expiry: type(uint64).max,
            enforceLimits: false,
            limits: emptyLimits,
            allowAnyCalls: true,
            allowedCalls: emptyScopes
        });
        _authorizeKey(msg.sender, keyId, signatureType, config);
        emit AdminKeyAuthorized(msg.sender, keyId);
    }

    function burnKeyAuthorizationWitness(bytes32 witness) external onlyRootTransaction {
        if (burnedWitnesses[msg.sender][witness]) revert KeyAuthorizationWitnessAlreadyBurned();
        burnedWitnesses[msg.sender][witness] = true;
        emit KeyAuthorizationWitnessBurned(msg.sender, witness);
    }

    function revokeKey(address keyId) external onlyRootTransaction {
        KeyInfo storage key = keys[msg.sender][keyId];
        if (key.expiry == 0) revert KeyNotFound();
        key.keyId = address(0);
        key.expiry = 0;
        key.enforceLimits = false;
        key.isRevoked = true;
        delete scopes[msg.sender][keyId];
        emit KeyRevoked(msg.sender, keyId);
    }

    function updateSpendingLimit(address keyId, address token, uint256 newLimit) external onlyRootTransaction {
        KeyInfo storage key = keys[msg.sender][keyId];
        if (key.isRevoked) revert KeyAlreadyRevoked();
        if (key.expiry == 0) revert KeyNotFound();
        key.enforceLimits = true;
        limits[msg.sender][keyId][token].remaining = newLimit;
        emit SpendingLimitUpdated(msg.sender, keyId, token, newLimit);
    }

    function setAllowedCalls(address keyId, CallScope[] calldata newScopes) external onlyRootTransaction {
        if (newScopes.length == 0) revert InvalidCallScope();
        _replaceAllowedCalls(msg.sender, keyId, newScopes, false);
    }

    function removeAllowedCalls(address keyId, address target) external onlyRootTransaction {
        ScopeState storage state = scopes[msg.sender][keyId];
        for (uint256 i; i < state.scopes.length; i++) {
            if (state.scopes[i].target == target) {
                state.scopes[i] = state.scopes[state.scopes.length - 1];
                state.scopes.pop();
                return;
            }
        }
    }

    function getKey(address account, address keyId) external view returns (KeyInfo memory) {
        KeyInfo memory key = keys[account][keyId];
        if (key.expiry == 0 || key.isRevoked) {
            return KeyInfo({
                signatureType: SignatureType.Secp256k1,
                keyId: address(0),
                expiry: 0,
                enforceLimits: false,
                isRevoked: key.isRevoked
            });
        }
        return key;
    }

    function getRemainingLimit(address account, address keyId, address token)
        external
        view
        returns (uint256 remaining)
    {
        (remaining,) = this.getRemainingLimitWithPeriod(account, keyId, token);
    }

    function getRemainingLimitWithPeriod(address account, address keyId, address token)
        external
        view
        returns (uint256 remaining, uint64 periodEnd)
    {
        KeyInfo memory key = keys[account][keyId];
        if (key.expiry == 0 || key.isRevoked || block.timestamp >= key.expiry) {
            return (0, 0);
        }
        LimitState memory limit = limits[account][keyId][token];
        return (limit.remaining, limit.periodEnd);
    }

    function getAllowedCalls(address account, address keyId)
        external
        view
        returns (bool isScoped, CallScope[] memory result)
    {
        KeyInfo memory key = keys[account][keyId];
        if (key.expiry == 0 || key.isRevoked || block.timestamp >= key.expiry) {
            return (true, new CallScope[](0));
        }

        ScopeState storage state = scopes[account][keyId];
        if (!state.isScoped) {
            return (false, new CallScope[](0));
        }

        result = new CallScope[](state.scopes.length);
        for (uint256 i; i < state.scopes.length; i++) {
            StoredCallScope storage storedScope = state.scopes[i];
            result[i].target = storedScope.target;
            result[i].selectorRules = new SelectorRule[](storedScope.selectorRules.length);

            for (uint256 j; j < storedScope.selectorRules.length; j++) {
                StoredSelectorRule storage storedRule = storedScope.selectorRules[j];
                result[i].selectorRules[j].selector = storedRule.selector;
                result[i].selectorRules[j].recipients = new address[](storedRule.recipients.length);

                for (uint256 k; k < storedRule.recipients.length; k++) {
                    result[i].selectorRules[j].recipients[k] = storedRule.recipients[k];
                }
            }
        }

        return (true, result);
    }

    function isKeyAuthorizationWitnessBurned(address account, bytes32 witness) external view returns (bool) {
        return burnedWitnesses[account][witness];
    }

    function isAdminKey(address account, address keyId) external pure returns (bool) {
        return account == keyId;
    }

    function getTransactionKey() external pure returns (address) {
        return address(0);
    }

    function _authorizeKey(
        address account,
        address keyId,
        SignatureType signatureType,
        KeyRestrictions memory config
    ) internal {
        if (keyId == address(0)) revert ZeroPublicKey();
        KeyInfo storage existing = keys[account][keyId];
        if (existing.isRevoked) revert KeyAlreadyRevoked();
        if (existing.expiry != 0) revert KeyAlreadyExists();
        if (config.expiry <= block.timestamp) revert ExpiryInPast();
        if (config.allowAnyCalls && config.allowedCalls.length != 0) revert InvalidCallScope();

        keys[account][keyId] = KeyInfo({
            signatureType: signatureType,
            keyId: keyId,
            expiry: config.expiry,
            enforceLimits: config.enforceLimits,
            isRevoked: false
        });

        if (config.enforceLimits) {
            for (uint256 i; i < config.limits.length; i++) {
                uint64 periodEnd = config.limits[i].period == 0
                    ? 0
                    : uint64(block.timestamp + config.limits[i].period);
                limits[account][keyId][config.limits[i].token] =
                    LimitState({remaining: config.limits[i].amount, periodEnd: periodEnd});
            }
        }

        _replaceAllowedCalls(account, keyId, config.allowedCalls, config.allowAnyCalls);
        emit KeyAuthorized(account, keyId, uint8(signatureType), config.expiry);
    }

    function _replaceAllowedCalls(
        address account,
        address keyId,
        CallScope[] memory newScopes,
        bool allowAnyCalls
    ) internal {
        ScopeState storage state = scopes[account][keyId];
        delete state.scopes;
        state.isScoped = !allowAnyCalls;

        for (uint256 i; i < newScopes.length; i++) {
            StoredCallScope storage storedScope = state.scopes.push();
            storedScope.target = newScopes[i].target;

            for (uint256 j; j < newScopes[i].selectorRules.length; j++) {
                StoredSelectorRule storage storedRule = storedScope.selectorRules.push();
                storedRule.selector = newScopes[i].selectorRules[j].selector;

                for (uint256 k; k < newScopes[i].selectorRules[j].recipients.length; k++) {
                    storedRule.recipients.push(newScopes[i].selectorRules[j].recipients[k]);
                }
            }
        }
    }
}
